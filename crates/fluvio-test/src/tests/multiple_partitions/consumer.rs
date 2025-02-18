use std::collections::HashSet;
use fluvio_test_util::test_meta::environment::EnvDetail;
use fluvio_test_util::test_runner::test_driver::TestDriver;
use futures_lite::StreamExt;
use fluvio::Offset;

use super::MultiplePartitionTestCase;

pub async fn consumer_stream(test_driver: &TestDriver, option: MultiplePartitionTestCase) {
    let consumer = test_driver
        .get_all_partitions_consumer(&option.environment.topic_name())
        .await;
    let mut stream = consumer.stream(Offset::beginning()).await.unwrap();

    let mut index: i32 = 0;

    let mut set = HashSet::new();

    while let Some(Ok(record)) = stream.next().await {
        let value = String::from_utf8_lossy(record.value())
            .parse::<i32>()
            .expect("Unable to parse");
        println!("Consuming {:<5}: was consumed: {:?}", index, value);

        assert!((0..5000).contains(&value));

        set.insert(value);
        index += 1;
        if index == 5000 {
            break;
        }
    }
    assert_eq!(set.len(), 5000)
}
