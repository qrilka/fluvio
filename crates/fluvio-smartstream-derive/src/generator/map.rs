use quote::quote;
use proc_macro2::TokenStream;
use crate::SmartStreamFn;

pub fn generate_map_smartstream(func: &SmartStreamFn) -> TokenStream {
    let user_code = &func.func;
    let user_fn = &func.name;

    quote! {
        #user_code

        mod __system {
            #[no_mangle]
            #[allow(clippy::missing_safety_doc)]
            pub unsafe fn map(ptr: *mut u8, len: usize) -> i32 {
                use fluvio_smartstream::dataplane::smartstream::{
                    SmartStreamInput, SmartStreamInternalError,
                    SmartStreamRuntimeError, SmartStreamType, SmartStreamOutput,
                };
                use fluvio_smartstream::dataplane::core::{Encoder, Decoder};
                use fluvio_smartstream::dataplane::record::{Record, RecordData};

                // DECODING
                extern "C" {
                    fn copy_records(putr: i32, len: i32);
                }

                let input_data = Vec::from_raw_parts(ptr, len, len);
                let mut smartstream_input = SmartStreamInput::default();
                if let Err(_err) = Decoder::decode(&mut smartstream_input, &mut std::io::Cursor::new(input_data), 0) {
                    return SmartStreamInternalError::DecodingBaseInput as i32;
                }

                let records_input = smartstream_input.record_data;
                let mut records: Vec<Record> = vec![];
                if let Err(_err) = Decoder::decode(&mut records, &mut std::io::Cursor::new(records_input), 0) {
                    return SmartStreamInternalError::DecodingRecords as i32;
                };

                // PROCESSING
                let mut output = SmartStreamOutput {
                    successes: Vec::with_capacity(records.len()),
                    error: None,
                };

                for mut record in records.into_iter() {
                    let result = super:: #user_fn(&record);
                    match result {
                        Ok((maybe_key, value)) => {
                            record.key = maybe_key;
                            record.value = value;
                            output.successes.push(record);
                        }
                        Err(err) => {
                            let error = SmartStreamRuntimeError::new(
                                &record,
                                smartstream_input.base_offset,
                                SmartStreamType::Map,
                                err,
                            );
                            output.error = Some(error);
                            break;
                        }
                    }
                }

                // ENCODING
                let mut out = vec![];
                if let Err(_) = Encoder::encode(&mut output, &mut out, 0) {
                    return SmartStreamInternalError::EncodingOutput as i32;
                }

                let out_len = out.len();
                let ptr = out.as_mut_ptr();
                std::mem::forget(out);
                copy_records(ptr as i32, out_len as i32);
                output.successes.len() as i32
            }
        }
    }
}
