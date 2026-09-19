#[lenso::plugin(consumer, lifecycle)]
#[derive(Clone, Debug)]
struct Audit {}
impl lenso::Lifecycle for Audit {
    async fn activate(&self, _: lenso::ActivateContext) -> Result<(), lenso::RuntimeFailure> {
        eprintln!("Shared audit Plugin activated by explicit Root intent");
        Ok(())
    }
    async fn deactivate(&self, _: lenso::DeactivateContext) -> Result<(), lenso::RuntimeFailure> {
        eprintln!("Shared audit Plugin stopped");
        Ok(())
    }
}
