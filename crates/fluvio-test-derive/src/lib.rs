use proc_macro::TokenStream;
use proc_macro2::Span;

use fluvio_test_util::test_meta::derive_attr::TestRequirements;
use syn::{AttributeArgs, Ident, ItemFn, parse_macro_input};
use quote::quote;
use inflections::Inflect;
use rand::{thread_rng, Rng};
use rand::distributions::Alphanumeric;

/// This macro will allow a test writer to override
/// minimum Fluvio cluster requirements for a test
///
/// Supported keys:
/// * `min_spu` (default `1`)
/// * `topic_name` (default: `topic`)
/// * `timeout` (default: `3600` seconds)
/// * `cluster_type` (default: no cluster restrictions)
/// * `name` (default: uses function name)
/// * `async` (default: false)
///
/// #[fluvio_test(min_spu = 2)]
/// pub async fn run(client: Arc<Fluvio>, mut test_case: TestCase) {
///     println!("Test")
/// }
///
/// Will expand into the following pseudocode
///
/// pub async fn run(client: Fluvio, option: TestCase) {
///     1. Read in TestRequirements from macro attrs
///     2. Compare TestRequirements with the current TestCase
///         - Any test that doesn't meet explicit requirements, skip to 6
///     3. Customize environment per TestRequirements
///     4. Create a topic
///     5. Start a timed test with (a default 1 hour timeout)
///     6. Return a TestResult
/// }
#[proc_macro_attribute]
pub fn fluvio_test(args: TokenStream, input: TokenStream) -> TokenStream {
    let fn_test_reqs: TestRequirements =
        match TestRequirements::from_ast(parse_macro_input!(args as AttributeArgs)) {
            Ok(attr) => attr,
            Err(_err) => panic!("Parse failed"),
        };

    // Serializing to string to pass into quote! block
    let fn_test_reqs_str =
        serde_json::to_string(&fn_test_reqs).expect("Could not serialize test reqs");

    // Read the user test
    let fn_user_test = parse_macro_input!(input as ItemFn);

    // Add some random to the generated function names so
    // we can support using the macro multiple times in the same file
    let rand_string: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(7)
        .map(char::from)
        .collect();

    //println!("{}", rand_string);

    // If test name is given, then use that instead of the test's function name
    let test_name = if let Some(req_test_name) = &fn_test_reqs.test_name {
        req_test_name.to_string()
    } else {
        fn_user_test.sig.ident.to_string()
    };

    let test_driver_name = format!("{}_{}", test_name, &rand_string);
    let user_test_name = format!("ext_test_fn_{}", &rand_string);
    let requirements_name = format!("requirements_{}", &rand_string);
    let validate_name = format!("validate_subcommand_{}", &rand_string);

    let test_driver_iden = Ident::new(&test_driver_name, Span::call_site());
    let user_test_fn_iden = Ident::new(&user_test_name, Span::call_site());
    let requirements_fn_iden = Ident::new(&requirements_name, Span::call_site());
    let validate_sub_fn_iden = Ident::new(&validate_name, Span::call_site());

    // Enforce naming convention for converting dyn TestOption to concrete type
    let test_opt_ident = Ident::new(
        &format!("{}TestOption", &test_name).to_pascal_case(),
        Span::call_site(),
    );

    // Finally, the test body
    let test_body = &fn_user_test.block;

    // We need to conditionally generate test code (per test) based on the `async` keyword
    let maybe_async_test = if fn_test_reqs.r#async {
        quote! {
            fluvio_future::task::run_block_on(async {
                // Automatically connect to cluster before handing control to async test
                test_driver
                    .connect()
                    .await
                    .expect("Connecting to cluster failed");

                #test_body
            });
        }
    } else {
        quote! { #test_body }
    };

    let output_fn = quote! {

        pub fn #validate_sub_fn_iden(subcmd: Vec<String>) -> Box<dyn fluvio_test_util::test_meta::TestOption> {
            Box::new(#test_opt_ident::from_iter(subcmd))
        }

        pub fn #requirements_fn_iden() -> fluvio_test_util::test_meta::derive_attr::TestRequirements {
            serde_json::from_str(#fn_test_reqs_str).expect("Could not deserialize test reqs")
        }

        inventory::submit!{
            fluvio_test_util::test_runner::test_meta::FluvioTestMeta {
                name: #test_name.to_string(),
                test_fn: #test_driver_iden,
                validate_fn: #validate_sub_fn_iden,
                requirements: #requirements_fn_iden,
            }
        }

        pub fn #user_test_fn_iden(mut test_driver: fluvio_test_util::test_runner::test_driver::TestDriver, test_case: fluvio_test_util::test_meta::TestCase) {
            use fluvio_test_util::test_meta::environment::EnvDetail;
            #maybe_async_test
        }

        pub fn #test_driver_iden(mut test_driver: fluvio_test_util::test_runner::test_driver::TestDriver, mut test_case: fluvio_test_util::test_meta::TestCase) -> Result<fluvio_test_util::test_meta::test_result::TestResult, fluvio_test_util::test_meta::test_result::TestResult> {
            use fluvio_test_util::test_meta::test_result::TestResult;
            use fluvio_test_util::test_meta::environment::EnvDetail;
            use fluvio_test_util::test_meta::derive_attr::TestRequirements;
            use fluvio_test_util::test_meta::test_timer::TestTimer;
            use fluvio_test_util::test_runner::test_driver::TestDriver;
            use fluvio_test_util::test_runner::test_meta::FluvioTestMeta;
            use fluvio_test_util::async_process;

            let test_reqs : TestRequirements = serde_json::from_str(#fn_test_reqs_str).expect("Could not deserialize test reqs");

            // Customize test environment if it meets minimum requirements
            if TestDriver::is_env_acceptable(&test_reqs, &test_case) {

                // Test-level environment customizations from macro attrs
                FluvioTestMeta::customize_test(&test_reqs, &mut test_case);

                // Setup topics before starting test
                // Doing setup in another process to avoid async in parent process
                // Otherwise there is .await blocking in child processes if tests fork() too
                let topic_setup_wait = async_process!(async {
                    let mut test_driver_setup = test_driver.clone();
                    // Connect test driver to cluster before starting test
                    test_driver_setup.connect().await.expect("Unable to connect to cluster");

                    // Create topic before starting test
                    test_driver_setup.create_topic(&test_case.environment)
                        .await
                        .expect("Unable to create default topic");

                    // Disconnect test driver to cluster before starting test
                    test_driver_setup.disconnect();
                });

                let _ = topic_setup_wait.join().expect("Topic setup wait failed");

                 // Start a test timer for the user's test now that setup is done
                let mut test_timer = TestTimer::start();
                let (test_end, test_end_listener) = crossbeam_channel::unbounded();

                // Run the test in its own process
                // We're not using the `async_process` macro here
                // Only bc we are sending something to a channel in waiting thread
                let test_process = match fork::fork() {
                    Ok(fork::Fork::Parent(child_pid)) => child_pid,
                    Ok(fork::Fork::Child) => {
                        #user_test_fn_iden(test_driver, test_case);
                        std::process::exit(0);
                    }
                    Err(_) => panic!("Test fork failed"),
                };

                let test_wait = std::thread::spawn( move || {
                    let pid = nix::unistd::Pid::from_raw(test_process.clone());
                    match nix::sys::wait::waitpid(pid, None) {
                        Ok(status) => {
                            tracing::debug!("[main] Test exited with status {:?}", status);

                            // Send something through the channel to signal test completion
                            test_end.send(()).unwrap();
                        }
                        Err(err) => panic!("[main] Test failed: {}", err),
                    }
                });

                // All of this is to handle test timeout
                let mut sel = crossbeam_channel::Select::new();
                let test_thread_done = sel.recv(&test_end_listener);
                let thread_selector = sel.select_timeout(test_case.environment.timeout());

                match thread_selector {
                    Err(_) => {
                        // Need to catch this panic to kill test parent process + all child processes
                        panic!("Test timeout met: {:?}", test_case.environment.timeout());
                    },
                    Ok(thread_selector) => match thread_selector.index() {
                        i if i == test_thread_done => {
                            // This is needed to let crossbeam select channel know we selected an operation, or it'll panic
                            let _ = thread_selector.recv(&test_end_listener);

                            // Stop the test timer before reporting
                            test_timer.stop();

                            Ok(TestResult {
                                success: true,
                                duration: test_timer.duration(),
                                ..std::default::Default::default()
                            })

                        }
                        _ => unreachable!()
                    }
                }
            } else {
                println!("Test skipped...");

                Ok(TestResult {
                    success: true,
                    duration: std::time::Duration::new(0, 0),
                    ..std::default::Default::default()
                })
            }
        }

    };

    output_fn.into()
}
