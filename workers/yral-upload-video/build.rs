use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    tonic_build::compile_protos("contracts/projects/warehouse_events/warehouse_events.proto")?;

    Ok(())
}
