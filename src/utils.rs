use tokio::{io as tio, process::Command};

pub async fn get_program_version(program: &str) -> tio::Result<Vec<u8>> {
    let output = Command::new(program).arg("--version").output().await?;
    Ok(output.stdout)
}

pub async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            println!("+ Shutdown in recv: ");
        },
        _ = terminate => {
            println!("+ Shutdown recv: ");
        },
    }
}
