#![deny(clippy::all)]

use napi_derive::napi;

#[napi]
pub fn print_message(message: String) -> String {
    println!("Received from TypeScript: {}", message);
    format!("Rust received: {}", message)
}
