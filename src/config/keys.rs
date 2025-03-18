use ring::{rand, signature};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn generate_keypair() -> Result<(Vec<u8>, Vec<u8>), Box<dyn std::error::Error>> {
    let rng = rand::SystemRandom::new();
    let pkcs8_bytes = signature::Ed25519KeyPair::generate_pkcs8(&rng)?;
    
    let key_pair = signature::Ed25519KeyPair::from_pkcs8(pkcs8_bytes.as_ref())?;
    let public_key = key_pair.public_key().as_ref().to_vec();
    
    Ok((pkcs8_bytes.as_ref().to_vec(), public_key))
}

fn save_keypair(private_key: &[u8], public_key: &[u8], id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let key_dir = determine_key_directory()?;
    fs::create_dir_all(&key_dir)?;
    
    // Save private key with restricted permissions
    let private_key_path = key_dir.join(format!("{}.private.key", id));
    let mut file = File::create(&private_key_path)?;
    file.write_all(private_key)?;
    
    // Set permissions to read/write for owner only
    #[cfg(unix)]
    {
        let metadata = file.metadata()?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o600); // Owner read/write only
        file.set_permissions(permissions)?;
    }
    
    // Save public key
    let public_key_path = key_dir.join(format!("{}.public.key", id));
    let mut file = File::create(public_key_path)?;
    file.write_all(public_key)?;
    
    Ok(())
}

fn determine_key_directory() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    // Use platform-specific directories
    #[cfg(target_os = "linux")]
    {
        let key_dir = Path::new("/etc/warden/keys/");
        Ok(key_dir.to_path_buf())
    }
    
    #[cfg(target_os = "windows")]
    {
        // For Windows, use %ProgramData%\warden\keys
        let program_data = std::env::var("ProgramData")?;
        let key_dir = Path::new(&program_data).join("warden").join("keys");
        Ok(key_dir)
    }
    
    #[cfg(target_os = "macos")]
    {
        // For macOS, use /Library/Application Support/warden/keys
        let key_dir = Path::new("/Library/Application Support/warden/keys");
        Ok(key_dir.to_path_buf())
    }
    
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        // Fallback
        let key_dir = Path::new("/etc/warden/keys/");
        Ok(key_dir.to_path_buf())
    }
}