# Simulator

EVM transaction simulator for testing token swaps with manipulated token balances. Built in Rust, exposed to Node.js via NAPI.

## Features

- Automatic balance slot discovery for any ERC20 token
- Dual simulation: `eth_callMany` RPC with REVM fallback
- Per-chain database caching
- TypeScript support

## Build

```bash
npm install
npm run build        # Release
npm run build:debug  # Debug
```

## Usage

```typescript
import { Simulator, SimulationParams } from 'simulator';

const simulator = new Simulator();

const params: SimulationParams = {
  user_address: "0x...",
  token_in_address: "0x...",
  to_address: "0x...",
  calldata: "0x...",
  amount_in: "1000000000000000000"
};

const result = await simulator.simulate(
  params,
  1,                             // Chain ID
  "https://rpc.example.com"
);

if (result.status === "simulation_success") {
  console.log("Output:", result.output);
} else if (result.status === "simulation_failed") {
  console.log("Reverted:", result.output);
} else {
  console.log("Error:", result.error);
}
```

### Result Types

- **SimulationSuccess**: `{ status: "simulation_success", output: string, rpc_err?: string }`
- **SimulationFailed**: `{ status: "simulation_failed", output: string, rpc_err?: string }`
- **Error**: `{ status: "error", error: string }`

### Concurrency Warning

⚠️ `simulate()` is **not safe for concurrent calls**. Always await each call before starting the next.

```typescript
// ❌ Don't
await Promise.all([simulator.simulate(...), simulator.simulate(...)]);

// ✅ Do
await simulator.simulate(...);
await simulator.simulate(...);
```

## How It Works

### Balance Slot Discovery

1. Inspects `balanceOf()` call to track all SLOAD operations
2. Tests each slot by setting a value and checking if balance changes

### Simulation

1. **RPC** (primary): Uses `eth_callMany` with state overrides
2. **REVM** (fallback): Local simulation if RPC fails

Both methods:
- Manipulate token balance at discovered slot
- Execute approve transaction
- Execute target transaction

## Project Structure

- `src/lib.rs` - NAPI bindings
- `src/simulator.rs` - Core simulation logic
- `src/balance_slot.rs` - Balance slot discovery
- `src/eth_call_many.rs` - `eth_callMany` RPC client
- `artifacts/erc20.sol` - Solidity interfaces
