use dagger_sdk::connect;
use eyre::Result;

const RUST_CONTAINER: &str = "rust:latest";

#[tokio::main]
async fn main() -> Result<()> {
    connect(|client| async move {
        test(&client).await
    })
        .await?;

    Ok(())
}

async fn test(client: &dagger_sdk::Query) -> Result<()> {
    let output = client
        .container()
        .from(RUST_CONTAINER)
        .with_directory("/src", client.host().directory(".."))
        .with_workdir("/src")
        .with_exec(vec!["cargo", "test"])
        .stdout()
        .await?;

    println!("{}", output);
    println!("✅ Tests passed");
    Ok(())
}
