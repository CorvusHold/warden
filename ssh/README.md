# SSH

This module provides a simple SSH client for Corvus.

## Usage

``` rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tunnel = SSHTunnel::new(
        "ssh.example.com".to_string(), 
        "username".to_string(), 
        Some(22)
    )
    .with_private_key_path("/path/to/private/key".to_string());

    tunnel.forward_port(
        9000,       // Local port
        8080,       // Remote port
        "localhost".to_string()  // Remote host
    )?;

    Ok(())
}
```

## Features

- SSH tunneling
- Support for private key authentication
- Support for password authentication
- Support for port forwarding