# ICP CLI Examples

This directory contains a comprehensive collection of examples demonstrating various features
and usage patterns of ICP CLI. Each example is a complete, working project that you can use
as a starting point or reference for your own Internet Computer applications.

## Quick Start

To try any example:

1. **Copy the example** to your workspace:
   ```bash
   cp -r examples/icp-motoko my-project
   cd my-project
   ```

2. **Start local network** (in separate terminal):
   ```bash
   icp network start
   ```

3. **Deploy the canister**:
   ```bash
   icp deploy
   ```

4. **Interact with your canister**:
   ```bash
   icp canister call my-canister greet '("World")'
   ```

Happy building! 🚀
