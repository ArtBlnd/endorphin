use crate::HouseKeeperError;

trait HouseKeeperCommand {
    type Output;

    fn execute(&self) -> Result<Self::Output, HouseKeeperError>;
}
