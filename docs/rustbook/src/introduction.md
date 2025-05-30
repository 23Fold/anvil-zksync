# Introduction

The In-memory node uses an in-memory database for storing state information and simplified hashmaps for tracking blocks and transactions. 
In fork mode, it retrieves missing storage data from a remote source when not available locally.
Moreover it also uses the remote server (openchain) to resolve the ABI and topics to human readable names.

You can visit the `anvil-zksync` repository [here](https://github.com/matter-labs/anvil-zksync) to learn more.

Please keep in mind that `anvil-zksync` is still in its **alpha** stage, so some features might not be fully supported yet and may not work as fully intended. 
It is [open-sourced](https://github.com/matter-labs/anvil-zksync) and contributions are welcomed.