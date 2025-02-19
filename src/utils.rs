use tokio::{io as tio, process::Command};



pub async fn get_program_version(program: &str) -> tio::Result<Vec<u8>> {
    let output = Command::new(program).arg("--version").output().await?; 
    Ok(output.stdout)
}