//! Helper methods to display transaction data in more human readable way.
use crate::bootloader_debug::BootloaderDebug;
use crate::utils::{calculate_eth_cost, to_human_size};
use alloy::hex::ToHexExt;
use anvil_zksync_common::sh_println;
use anvil_zksync_config::utils::format_gwei;
use colored::Colorize;
use lazy_static::lazy_static;
use serde::Deserialize;
use std::fmt;
use std::{collections::HashMap, str};
use zksync_error::{documentation::Documented, CustomErrorMessage, NamedError};
use zksync_error_description::ErrorDocumentation;
use zksync_multivm::interface::VmExecutionResultAndLogs;
use zksync_types::{
    fee_model::FeeModelConfigV2, Address, ExecuteTransactionCommon, StorageLogWithPreviousValue,
    Transaction, H160, U256,
};

// @dev elected to have GasDetails struct as we can do more with it in the future
// We can provide more detailed understanding of gas errors and gas usage
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct GasDetails {
    total_gas_limit: U256,
    intrinsic_gas: U256,
    gas_for_validation: U256,
    gas_spent_on_compute: U256,
    gas_used: U256,
    bytes_published: u64,
    spent_on_pubdata: u64,
    gas_spent_on_bytecode_preparation: U256,
    refund_computed: U256,
    refund_by_operator: U256,
    required_overhead: U256,
    operator_overhead: U256,
    intrinsic_overhead: U256,
    overhead_for_length: U256,
    overhead_for_slot: U256,
    gas_per_pubdata: U256,
    total_gas_limit_from_user: U256,
    gas_spent_on_execution: U256,
    gas_limit_after_intrinsic: U256,
    gas_after_validation: U256,
    reserved_gas: U256,
}

/// Computes the gas details for the transaction to be displayed.
pub fn compute_gas_details(
    bootloader_debug: &BootloaderDebug,
    spent_on_pubdata: u64,
) -> GasDetails {
    let total_gas_limit = bootloader_debug
        .total_gas_limit_from_user
        .saturating_sub(bootloader_debug.reserved_gas);
    let intrinsic_gas = total_gas_limit - bootloader_debug.gas_limit_after_intrinsic;
    let gas_for_validation =
        bootloader_debug.gas_limit_after_intrinsic - bootloader_debug.gas_after_validation;
    let gas_spent_on_compute = bootloader_debug.gas_spent_on_execution
        - bootloader_debug.gas_spent_on_bytecode_preparation;
    let gas_used = intrinsic_gas
        + gas_for_validation
        + bootloader_debug.gas_spent_on_bytecode_preparation
        + gas_spent_on_compute;

    let bytes_published = spent_on_pubdata / bootloader_debug.gas_per_pubdata.as_u64();

    GasDetails {
        total_gas_limit,
        intrinsic_gas,
        gas_for_validation,
        gas_spent_on_compute,
        gas_used,
        bytes_published,
        spent_on_pubdata,
        gas_spent_on_bytecode_preparation: bootloader_debug.gas_spent_on_bytecode_preparation,
        refund_computed: bootloader_debug.refund_computed,
        refund_by_operator: bootloader_debug.refund_by_operator,
        required_overhead: bootloader_debug.required_overhead,
        operator_overhead: bootloader_debug.operator_overhead,
        intrinsic_overhead: bootloader_debug.intrinsic_overhead,
        overhead_for_length: bootloader_debug.overhead_for_length,
        overhead_for_slot: bootloader_debug.overhead_for_slot,
        gas_per_pubdata: bootloader_debug.gas_per_pubdata,
        total_gas_limit_from_user: bootloader_debug.total_gas_limit_from_user,
        gas_spent_on_execution: bootloader_debug.gas_spent_on_execution,
        gas_limit_after_intrinsic: bootloader_debug.gas_limit_after_intrinsic,
        gas_after_validation: bootloader_debug.gas_after_validation,
        reserved_gas: bootloader_debug.reserved_gas,
    }
}

/// Responsible for formatting the data in a structured log.
pub struct Formatter {
    sibling_stack: Vec<bool>,
}

impl Default for Formatter {
    fn default() -> Self {
        Self::new()
    }
}

impl Formatter {
    /// Creates a new formatter with an empty sibling stack.
    pub fn new() -> Self {
        Formatter {
            sibling_stack: Vec::new(),
        }
    }
    /// Logs a section with a title, applies a scoped function, and manages sibling hierarchy.
    pub fn section<F>(&mut self, title: &str, is_last_sibling: bool, f: F)
    where
        F: FnOnce(&mut Self),
    {
        self.format_log(is_last_sibling, title);
        self.enter_scope(is_last_sibling);
        f(self);
        self.exit_scope();
    }
    /// Logs a key-value item as part of the formatted output.
    pub fn item(&mut self, is_last_sibling: bool, key: &str, value: &str) {
        self.format_log(
            is_last_sibling,
            &format!("{}: {}", key.bold(), value.dimmed()),
        );
    }
    /// Enters a new scope for nested logging, tracking sibling relationships.
    pub fn enter_scope(&mut self, has_more_siblings: bool) {
        self.sibling_stack.push(has_more_siblings);
    }
    /// Exits the current logging scope, removing the last sibling marker.
    pub fn exit_scope(&mut self) {
        self.sibling_stack.pop();
    }
    /// Logs a formatted message with a hierarchical prefix.
    pub fn format_log(&self, is_last_sibling: bool, message: &str) {
        let prefix = build_prefix(&self.sibling_stack, is_last_sibling);
        sh_println!("{}{}", prefix, message);
    }
    /// Logs a formatted error message with a hierarchical prefix.
    pub fn format_error(&self, is_last_sibling: bool, message: &str) {
        let prefix = build_prefix(&self.sibling_stack, is_last_sibling);
        sh_println!("{}", format!("{}{}", prefix, message).red());
    }
    /// Prints gas details for the transaction in a structured log.
    pub fn print_gas_details(
        &mut self,
        gas_details: &GasDetails,
        fee_model_config: &FeeModelConfigV2,
    ) {
        let GasDetails {
            total_gas_limit,
            intrinsic_gas,
            gas_for_validation,
            gas_spent_on_compute,
            gas_used,
            bytes_published,
            spent_on_pubdata,
            gas_spent_on_bytecode_preparation,
            refund_computed,
            refund_by_operator,
            required_overhead: _required_overhead,
            operator_overhead,
            intrinsic_overhead,
            overhead_for_length,
            overhead_for_slot,
            gas_per_pubdata,
            total_gas_limit_from_user,
            ..
        } = *gas_details;

        self.section("[Gas Details]", true, |gas_details_section| {
            let mut total_items = 0;
            let mut warnings = Vec::new();

            // Prepare warnings
            if refund_computed != refund_by_operator {
                warnings.push(format!(
                    "WARNING: Refund by VM: {}, but operator refunded: {}",
                    to_human_size(refund_computed),
                    to_human_size(refund_by_operator)
                ));
            }

            if total_gas_limit_from_user != total_gas_limit {
                warnings.push(format!(
                    "WARNING: User provided more gas ({}), but system had a lower max limit.",
                    to_human_size(total_gas_limit_from_user)
                ));
            }

            // Calculate total items under [Gas Details]
            total_items += 1; // Gas Summary
            total_items += warnings.len(); // Warnings
            total_items += 1; // Execution Gas Breakdown
            total_items += 1; // Transaction Setup Cost Breakdown
            total_items += 1; // L1 Publishing Costs
            total_items += 1; // Block Contribution

            let mut item_index = 0;

            // Gas Summary
            let is_last_sibling = item_index == total_items - 1;
            gas_details_section.section("Gas Summary", is_last_sibling, |gas_summary_section| {
                let items = vec![
                    ("Limit", to_human_size(total_gas_limit)),
                    ("Used", to_human_size(gas_used)),
                    ("Refunded", to_human_size(refund_by_operator)),
                    ("Paid:", to_human_size(total_gas_limit - refund_by_operator)),
                ];

                let num_items = items.len();
                for (i, (key, value)) in items.into_iter().enumerate() {
                    let is_last_item = i == num_items - 1;
                    gas_summary_section.item(is_last_item, key, &value);
                }
            });
            item_index += 1;

            // warnings
            for warning in warnings {
                let is_last_sibling = item_index == total_items - 1;
                gas_details_section.format_error(is_last_sibling, &warning);
                item_index += 1;
            }

            // Execution Gas Breakdown
            let is_last_sibling = item_index == total_items - 1;
            gas_details_section.section(
                "Execution Gas Breakdown",
                is_last_sibling,
                |execution_breakdown_section| {
                    let gas_breakdown_items = vec![
                        (
                            "Transaction Setup",
                            intrinsic_gas,
                            intrinsic_gas * 100 / gas_used,
                        ),
                        (
                            "Bytecode Preparation",
                            gas_spent_on_bytecode_preparation,
                            gas_spent_on_bytecode_preparation * 100 / gas_used,
                        ),
                        (
                            "Account Validation",
                            gas_for_validation,
                            gas_for_validation * 100 / gas_used,
                        ),
                        (
                            "Computations (Opcodes)",
                            gas_spent_on_compute,
                            gas_spent_on_compute * 100 / gas_used,
                        ),
                    ];

                    let num_items = gas_breakdown_items.len();
                    for (i, (description, amount, percentage)) in
                        gas_breakdown_items.iter().enumerate()
                    {
                        let is_last_item = i == num_items - 1;
                        execution_breakdown_section.item(
                            is_last_item,
                            description,
                            &format!("{} gas ({:>2}%)", to_human_size(*amount), percentage),
                        );
                    }
                },
            );
            item_index += 1;

            // Transaction Setup Cost Breakdown
            let is_last_sibling = item_index == total_items - 1;
            gas_details_section.section(
                "Transaction Setup Cost Breakdown",
                is_last_sibling,
                |transaction_setup_section| {
                    let items = vec![
                        (
                            "Total Setup Cost",
                            format!("{} gas", to_human_size(intrinsic_gas)),
                        ),
                        (
                            "Fixed Cost",
                            format!(
                                "{} gas ({:>2}%)",
                                to_human_size(intrinsic_overhead),
                                intrinsic_overhead * 100 / intrinsic_gas
                            ),
                        ),
                        (
                            "Operator Cost",
                            format!(
                                "{} gas ({:>2}%)",
                                to_human_size(operator_overhead),
                                operator_overhead * 100 / intrinsic_gas
                            ),
                        ),
                    ];

                    let num_items = items.len();
                    for (i, (key, value)) in items.into_iter().enumerate() {
                        let is_last_item = i == num_items - 1;
                        transaction_setup_section.item(is_last_item, key, &value);
                    }
                },
            );
            item_index += 1;

            // L1 Publishing Costs
            let is_last_sibling = item_index == total_items - 1;
            gas_details_section.section(
                "L1 Publishing Costs",
                is_last_sibling,
                |l1_publishing_section| {
                    let items = vec![
                        (
                            "Published",
                            format!("{} bytes", to_human_size(bytes_published.into())),
                        ),
                        (
                            "Cost per Byte",
                            format!("{} gas", to_human_size(gas_per_pubdata)),
                        ),
                        (
                            "Total Gas Cost",
                            format!("{} gas", to_human_size(spent_on_pubdata.into())),
                        ),
                    ];

                    let num_items = items.len();
                    for (i, (key, value)) in items.into_iter().enumerate() {
                        let is_last_item = i == num_items - 1;
                        l1_publishing_section.item(is_last_item, key, &value);
                    }
                },
            );
            item_index += 1;

            // Block Contribution
            let is_last_sibling = item_index == total_items - 1;
            gas_details_section.section("Block Contribution", is_last_sibling, |block_section| {
                let full_block_cost = gas_per_pubdata * fee_model_config.batch_overhead_l1_gas;

                let items = vec![
                    (
                        "Length Overhead",
                        format!("{} gas", to_human_size(overhead_for_length)),
                    ),
                    (
                        "Slot Overhead",
                        format!("{} gas", to_human_size(overhead_for_slot)),
                    ),
                    (
                        "Full Block Cost",
                        format!("~{} L2 gas", to_human_size(full_block_cost)),
                    ),
                ];

                let num_items = items.len();
                for (i, (key, value)) in items.into_iter().enumerate() {
                    let is_last_item = i == num_items - 1;
                    block_section.item(is_last_item, key, &value);
                }
            });
        });
    }
    /// Prints the storage logs of the system in a structured log.
    pub fn print_storage_logs(
        &mut self,
        log_query: &StorageLogWithPreviousValue,
        pubdata_bytes: Option<PubdataBytesInfo>,
        log_index: usize,
        is_last: bool,
    ) {
        self.section(&format!("Log #{}", log_index), is_last, |log_section| {
            let mut items = vec![
                ("Kind", format!("{:?}", log_query.log.kind)),
                (
                    "Address",
                    address_to_human_readable(*log_query.log.key.address())
                        .unwrap_or_else(|| format!("{:?}", log_query.log.key.address())),
                ),
                ("Key", format!("{:#066x}", log_query.log.key.key())),
                ("Read Value", format!("{:#066x}", log_query.previous_value)),
            ];

            if log_query.log.is_write() {
                items.push(("Written Value", format!("{:#066x}", log_query.log.value)));
            }

            let pubdata_bytes_str = pubdata_bytes
                .map(|p| format!("{}", p))
                .unwrap_or_else(|| "None".to_string());
            items.push(("Pubdata Bytes", pubdata_bytes_str));

            let num_items = items.len();
            for (i, (key, value)) in items.iter().enumerate() {
                let is_last_item = i == num_items - 1;
                log_section.item(is_last_item, key, value);
            }
        });
    }
    /// Prints the VM execution results in a structured log.
    pub fn print_vm_details(&mut self, result: &VmExecutionResultAndLogs) {
        self.section("[VM Execution Results]", true, |section| {
            let stats = [
                (
                    "Cycles Used",
                    to_human_size(result.statistics.cycles_used.into()),
                ),
                (
                    "Computation Gas Used",
                    to_human_size(result.statistics.computational_gas_used.into()),
                ),
                (
                    "Contracts Used",
                    to_human_size(result.statistics.contracts_used.into()),
                ),
            ];

            for (key, value) in stats.iter() {
                section.item(false, key, value);
            }

            // Handle execution outcome
            match &result.result {
                zksync_multivm::interface::ExecutionResult::Success { .. } => {
                    section.item(true, "Execution Outcome", "Success");
                }
                zksync_multivm::interface::ExecutionResult::Revert { output } => {
                    section.item(false, "Execution Outcome", "Failure");
                    section.format_error(
                        true,
                        &format!("Revert Reason: {}", output.to_user_friendly_string()),
                    );
                }
                zksync_multivm::interface::ExecutionResult::Halt { reason } => {
                    section.item(false, "Execution Outcome", "Failure");
                    section.format_error(true, &format!("Halt Reason: {}", reason));
                }
            }
        });
    }
}
// Builds the branched prefix for the structured logs.
fn build_prefix(sibling_stack: &[bool], is_last_sibling: bool) -> String {
    let mut prefix = String::new();
    if !sibling_stack.is_empty() {
        for &is_last in sibling_stack {
            if !is_last {
                prefix.push_str("│   ");
            } else {
                prefix.push_str("    ");
            }
        }
        let branch = if is_last_sibling {
            "└─ "
        } else {
            "├─ "
        };
        prefix.push_str(branch);
    }
    prefix
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub enum ContractType {
    System,
    Precompile,
    Popular,
    Unknown,
}

#[derive(Debug, Deserialize, Clone)]
pub struct KnownAddress {
    address: H160,
    name: String,
    contract_type: ContractType,
}

lazy_static! {
    /// Loads the known contact addresses from the JSON file.
    static ref KNOWN_ADDRESSES: HashMap<H160, KnownAddress> = {
        let json_value = serde_json::from_slice(include_bytes!("data/address_map.json")).unwrap();
        let pairs: Vec<KnownAddress> = serde_json::from_value(json_value).unwrap();

        pairs
            .into_iter()
            .map(|entry| (entry.address, entry))
            .collect()
    };
}

fn format_known_address(address: H160) -> Option<String> {
    KNOWN_ADDRESSES.get(&address).map(|known_address| {
        let name = match known_address.contract_type {
            ContractType::System => known_address.name.bold().bright_blue().to_string(),
            ContractType::Precompile => known_address.name.bold().magenta().to_string(),
            ContractType::Popular => known_address.name.bold().bright_green().to_string(),
            ContractType::Unknown => known_address.name.dimmed().to_string(),
        };

        let formatted_address = format!("{:#x}", address).dimmed();
        format!("{}{}{}", name, "@".dimmed(), formatted_address)
    })
}

fn address_to_human_readable(address: H160) -> Option<String> {
    format_known_address(address)
}

/// Amount of pubdata that given write has cost.
/// Used when displaying Storage Logs.
pub enum PubdataBytesInfo {
    // This slot is free
    FreeSlot,
    // This slot costs this much.
    Paid(u32),
    // This happens when we already paid a little for this slot in the past.
    // This slots costs additional X, the total cost is Y.
    AdditionalPayment(u32, u32),
    // We already paid for this slot in this transaction.
    PaidAlready,
}

impl std::fmt::Display for PubdataBytesInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PubdataBytesInfo::FreeSlot => write!(f, "Free Slot (no cost)"),
            PubdataBytesInfo::Paid(cost) => {
                write!(f, "Paid: {} bytes", to_human_size((*cost).into()))
            }
            PubdataBytesInfo::AdditionalPayment(additional_cost, total_cost) => write!(
                f,
                "Additional Payment: {} bytes (Total: {} bytes)",
                to_human_size((*additional_cost).into()),
                to_human_size((*total_cost).into())
            ),
            PubdataBytesInfo::PaidAlready => write!(f, "Already Paid (no additional cost)"),
        }
    }
}

impl PubdataBytesInfo {
    // Whether the slot incurs any cost
    pub fn does_cost(&self) -> bool {
        match self {
            PubdataBytesInfo::FreeSlot => false,
            PubdataBytesInfo::Paid(_) => true,
            PubdataBytesInfo::AdditionalPayment(_, _) => true,
            PubdataBytesInfo::PaidAlready => false,
        }
    }
}

// @dev Separate from Formatter as it does not make use of structured log format.
/// Print the transaction summary.
pub fn print_transaction_summary(
    l2_gas_price: u64,
    tx: &Transaction,
    tx_result: &VmExecutionResultAndLogs,
    status: &str,
) {
    // Calculate used and refunded gas
    let used_gas = tx.gas_limit() - tx_result.refunds.gas_refunded;
    let paid_in_eth = calculate_eth_cost(l2_gas_price, used_gas.as_u64());
    let refunded_gas = tx_result.refunds.gas_refunded;
    let refunded_in_eth = calculate_eth_cost(l2_gas_price, refunded_gas);

    let emoji = match status {
        "SUCCESS" => "✅",
        "FAILED" => "❌",
        "HALTED" => "⏸️",
        _ => "⚠️",
    };

    sh_println!(
        r#"
{} [{}] Hash: {tx_hash:?}
Initiator: {initiator:?}
Payer: {payer:?}
Gas Limit: {gas_limit} | Used: {used} | Refunded: {refunded}
Paid: {paid:.10} ETH ({} gas * {l2_gas_price_fmt})
Refunded: {:.10} ETH
"#,
        emoji,
        status,
        used_gas,
        refunded_in_eth,
        tx_hash = tx.hash(),
        initiator = tx.initiator_account(),
        payer = tx.payer(),
        gas_limit = to_human_size(tx.gas_limit()),
        used = to_human_size(used_gas),
        refunded = to_human_size(tx_result.refunds.gas_refunded.into()),
        paid = paid_in_eth,
        l2_gas_price_fmt = format_gwei(l2_gas_price.into())
    );
    sh_println!("");
}

/// Encapsulates the execution error report.
#[derive(Debug)]
pub struct ExecutionErrorReport<'a, E> {
    error: &'a E,
    tx: Option<&'a Transaction>,
}

impl<'a, E> ExecutionErrorReport<'a, E>
where
    E: NamedError + CustomErrorMessage + Documented<Documentation = &'static ErrorDocumentation>,
{
    pub fn new(error: &'a E, tx: Option<&'a Transaction>) -> Self {
        Self { error, tx }
    }

    /// Returns the error details.
    fn error_report(&self) -> String {
        let mut out = String::new();
        let error_msg = self.error.get_message();

        out += &format!("{}: {}\n", "error".red().bold(), error_msg.red());
        out += "    |\n";
        let doc = match self.error.get_documentation() {
            Ok(opt) => opt,
            Err(e) => {
                tracing::info!("Failed to get error documentation: {}", e);
                None
            }
        };
        let summary = doc
            .as_ref()
            .map_or("An unknown error occurred", |d| d.summary.as_str());
        out += &format!("    = {} {}\n", "error:".bright_red(), summary);
        out
    }

    /// Returns the transaction details if available.
    fn tx_details(&self) -> String {
        let mut out = String::new();
        if let Some(tx) = self.tx {
            out += "    | \n";
            out += &format!("    | {}\n", "Transaction details:".cyan());
            out += &format!("    |   Transaction Type: {:?}\n", tx.tx_format());
            if let Some(nonce) = tx.nonce() {
                out += &format!("    |   Nonce: {}\n", nonce);
            }
            if let Some(contract_address) = tx.recipient_account() {
                out += &format!("    |   To: {:?}\n", contract_address);
            }
            out += &format!("    |   From: {:?}\n", tx.initiator_account());
            if let ExecuteTransactionCommon::L2(l2_tx) = &tx.common_data {
                if let Some(input_data) = &l2_tx.input {
                    let hex_data = input_data.data.encode_hex();
                    out += &format!("    |   Input Data: 0x{}\n", hex_data);
                    out += &format!("    |   Hash: {:?}\n", tx.hash());
                }
            }
            out += &format!("    |   Gas Limit: {}\n", tx.gas_limit());
            out += &format!("    |   Gas Price: {}\n", format_gwei(tx.max_fee_per_gas()));
            out += &format!(
                "    |   Gas Per Pubdata Limit: {}\n",
                tx.gas_per_pubdata_byte_limit()
            );

            // Log paymaster details if available.
            if let ExecuteTransactionCommon::L2(l2_tx) = &tx.common_data {
                let paymaster_address = l2_tx.paymaster_params.paymaster;
                let paymaster_input = &l2_tx.paymaster_params.paymaster_input;
                if paymaster_address != Address::zero() || !paymaster_input.is_empty() {
                    out += &format!("    | {}\n", "Paymaster details:".cyan());
                    out += &format!("    |   Paymaster Address: {:?}\n", paymaster_address);
                    let paymaster_input_str = if paymaster_input.is_empty() {
                        "None".to_string()
                    } else {
                        paymaster_input.encode_hex()
                    };
                    out += &format!("    |   Paymaster Input: 0x{}\n", paymaster_input_str);
                }
            }
        }
        out
    }

    /// Returns documentation details including likely causes, fixes, and references.
    fn docs(&self) -> String {
        let mut out = String::new();
        if let Ok(Some(doc)) = self.error.get_documentation() {
            if !doc.likely_causes.is_empty() {
                out += "    | \n";
                out += &format!("    | {}\n", "Likely causes:".cyan());
                for cause in &doc.likely_causes {
                    out += &format!("    |   - {}\n", cause.cause);
                }
                // Collect fixes.
                let all_fixes: Vec<&String> = doc
                    .likely_causes
                    .iter()
                    .flat_map(|cause| &cause.fixes)
                    .collect();
                if !all_fixes.is_empty() {
                    out += "    | \n";
                    out += &format!("    | {}\n", "Possible fixes:".green().bold());
                    for fix in &all_fixes {
                        out += &format!("    |   - {}\n", fix);
                    }
                }
                // Collect references.
                let all_references: Vec<&String> = doc
                    .likely_causes
                    .iter()
                    .flat_map(|cause| &cause.references)
                    .collect();
                if !all_references.is_empty() {
                    out += &format!(
                        "\n{} \n",
                        "For more information about this error, visit:"
                            .cyan()
                            .bold()
                    );
                    for reference in &all_references {
                        out += &format!("  - {}\n", reference.underline());
                    }
                }
            }
            out += "    |\n";
            out += &format!("{} {}\n", "note:".blue(), doc.description);
        }
        out += &format!(
            "{} transaction execution halted due to the above error\n",
            "error:".red()
        );
        out
    }

    /// Combines all parts of the error report into one string.
    pub fn report(&self) -> String {
        let mut out = String::new();
        out += &self.error_report();
        out += &self.tx_details();
        out += &self.docs();
        out
    }
}

/// Implementing Display allows the error report to be used in formatting macros.
impl<E> fmt::Display for ExecutionErrorReport<'_, E>
where
    E: NamedError + CustomErrorMessage + Documented<Documentation = &'static ErrorDocumentation>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.report())
    }
}
