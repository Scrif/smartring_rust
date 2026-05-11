use anyhow::Result;
use smartring_core::client::Client;
use smartring_core::reboot::reboot_request;

pub async fn run(client: &Client) -> Result<()> {
    client.send_recv(reboot_request(), 0).await?;
    println!("Rebooting ring…");
    Ok(())
}
