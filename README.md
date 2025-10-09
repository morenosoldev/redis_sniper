This is a high-performance trade execution microservice built in Rust, designed for low-latency order execution on the Solana blockchain.

It forms the execution layer of a larger trading bot system — listening for buy and sell signals from Redis, and reliably executing those trades on Solana with retry and landing-rate optimization mechanisms.

The trading strategy logic (signal generation, risk management, etc.) lives in a separate service.
This microservice focuses purely on execution and reliability.
