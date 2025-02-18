use fluvio_test_derive::fluvio_test;
#[warn(unused_imports)]
use fluvio_test_util::test_meta::TestCase;
use structopt::StructOpt;
use std::any::Any;
use fluvio_test_util::test_meta::TestOption;

#[derive(Debug, Clone, StructOpt, Default, PartialEq)]
#[structopt(name = "Fluvio Test Example")]
pub struct TestNameTestOption {}

impl TestOption for TestNameTestOption {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[fluvio_test(name = "test_name")]
pub fn run(mut test_driver: TestDriver, test_case: TestCase) {
}

fn main() {
}

