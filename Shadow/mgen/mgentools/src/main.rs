use std::collections::HashMap;
use std::{error::Error, fs::File, path::Path, process};

use serde::{ser::SerializeSeq, Deserialize, Serializer};

// default shadow simulation start
// Sat Jan  1 12:00:00 AM UTC 2000
// expressed as a unix timestamp
const EXPERIMENT_START: f64 = 946684800.0;
const TIMEOUT: f64 = 30.0;

#[derive(Debug, Deserialize)]
/// A record from the mgen log file.
struct Record {
    _hr_time: String,
    time: f64, // FIXME: use a better type, either a real datetime or our own two ints
    user: String,
    group: String,
    action_data: Vec<String>,
}

/// Running values for a (user, group) tuple
struct RunningValue {
    sent_count: u32,
    sent_timeout: u32,
    incoming_receipt_count: u32,
    outgoing_receipt_count: u32,
    receipt_timeout: u32,
    running_rtt_mean: f64,
}

/// When a (user, group) starts sending messages back
type BootstrapTable = HashMap<(String, String), f64>;

struct Serializers<T: SerializeSeq> {
    rtt_all: T,
    rtt_timeout: T,
    rtt_mean: T,
    sent_count: T,
    timeout_by_send: T,
    timeout_by_receive: T,
}

fn process_log<T>(
    file: &Path,
    filter_time: f64,
    bootstrap_table: &BootstrapTable,
    serializers: &mut Serializers<T>,
) -> Result<(), Box<dyn Error>>
where
    T: SerializeSeq,
{
    let mut sent_times: HashMap<(String, String, u32), f64> = HashMap::new();
    let mut running_values: HashMap<(String, String), RunningValue> = HashMap::new();
    let file = File::open(file)?;

    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(file);
    for result in rdr.deserialize() {
        let record: Record = match result {
            Ok(record) => record,
            Err(_e) => {
                //eprintln!("bad record: {:?}", e);
                continue;
            }
        };

        if record.time <= filter_time {
            continue;
        }

        match record.action_data[0].as_str() {
            "send" => {
                let id = record.action_data[4].parse()?;
                sent_times.insert((record.user.clone(), record.group.clone(), id), record.time);
                running_values
                    .entry((record.user, record.group))
                    .and_modify(|running_value| running_value.sent_count += 1)
                    .or_insert(RunningValue {
                        sent_count: 1,
                        sent_timeout: 0,
                        incoming_receipt_count: 0,
                        outgoing_receipt_count: 0,
                        receipt_timeout: 0,
                        running_rtt_mean: 0.0,
                    });
            }
            "receive" => {
                if record.action_data[4] != "receipt" {
                    continue;
                }

                let sender = &record.action_data[3];
                let id = record.action_data[5].parse()?;
                let key = (sender.to_string(), record.group.clone());
                let bootstrap_time = bootstrap_table.get(&key).unwrap_or_else(|| {
                    panic!("could not find key {:?}", key);
                });

                let key = (record.user, key.1, id);
                let Some(sent_time) = sent_times.get(&key) else {
                    // this should never happen in the client-server case,
                    // but we filter out early conversation in the p2p case
                    //eprintln!("receipt for unknown message: {:?}", key);
                    //panic!();
                    continue;
                };

                if bootstrap_time > sent_time {
                    // the message was sent while the recipient was still bootstrapping,
                    // don't count its receipt towards the rtt stats
                    continue;
                }

                let rtt: f64 = record.time - sent_time;
                serializers
                    .rtt_all
                    .serialize_element(&rtt)
                    .unwrap_or_else(|e| {
                        panic!("unable to serialize rtt: {:?}", e);
                    });
                if rtt <= TIMEOUT {
                    serializers
                        .rtt_timeout
                        .serialize_element(&rtt)
                        .unwrap_or_else(|e| {
                            panic!("unable to serialize rtt: {:?}", e);
                        });
                }

                let key = (key.0, key.1);
                running_values.entry(key).and_modify(|running_value| {
                    running_value.incoming_receipt_count += 1;
                    if rtt > TIMEOUT {
                        running_value.sent_timeout += 1;
                    }
                    running_value.running_rtt_mean = running_value.running_rtt_mean
                        + (rtt - running_value.running_rtt_mean)
                            / (running_value.incoming_receipt_count as f64);
                });

                let key = (sender.to_string(), record.group);
                let receipt_sender = running_values.entry(key).or_insert(RunningValue {
                    sent_count: 0,
                    sent_timeout: 0,
                    incoming_receipt_count: 0,
                    outgoing_receipt_count: 0,
                    receipt_timeout: 0,
                    running_rtt_mean: 0.0,
                });

                receipt_sender.outgoing_receipt_count += 1;
                if rtt > TIMEOUT {
                    receipt_sender.receipt_timeout += 1;
                }
            }
            _ => (),
        }
    }

    for value in running_values.into_values() {
        serializers
            .rtt_mean
            .serialize_element(&value.running_rtt_mean)
            .unwrap_or_else(|e| {
                panic!("unable to serialize rtt mean: {:?}", e);
            });
        serializers
            .sent_count
            .serialize_element(&value.sent_count)
            .unwrap_or_else(|e| {
                panic!("unable to serialize rtt count: {:?}", e);
            });
        if value.incoming_receipt_count != 0 {
            serializers
                .timeout_by_send
                .serialize_element(
                    &(value.sent_timeout as f64 / value.incoming_receipt_count as f64),
                )
                .unwrap_or_else(|e| {
                    panic!("unable to serialize rtt count: {:?}", e);
                });
        } else {
            assert_eq!(value.sent_timeout, value.incoming_receipt_count);
        }
        if value.outgoing_receipt_count != 0 {
            serializers
                .timeout_by_receive
                .serialize_element(
                    &(value.receipt_timeout as f64 / value.outgoing_receipt_count as f64),
                )
                .unwrap_or_else(|e| {
                    panic!("unable to serialize rtt count: {:?}", e);
                });
        } else {
            assert_eq!(value.receipt_timeout, value.outgoing_receipt_count);
        }
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
struct ConversationConfig {
    group: String,
    bootstrap: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct UserConfig {
    user: String,
    bootstrap: f64,
    conversations: Vec<ConversationConfig>,
}

fn parse_time(time: &str) -> Result<f64, Box<dyn Error>> {
    let suffix = time.as_bytes()[time.len() - 1];
    Ok(match suffix {
        b's' => time[..time.len() - 1].parse::<f64>()?, // seconds
        b'm' => time[..time.len() - 1].parse::<f64>()? * 60.0, // minutes
        b'h' => time[..time.len() - 1].parse::<f64>()? * 60.0 * 60.0, // hours
        _ => time.parse::<f64>()?, // default is seconds, anything else is an error
    })
}

#[derive(Debug, Deserialize)]
struct ShadowGeneralConfig {
    bootstrap_end_time: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ShadowProcessArgs {
    List(Vec<String>),
    Command(String),
}

#[derive(Debug, Deserialize)]
struct ShadowProcessConfig {
    args: ShadowProcessArgs,
    path: String,
    start_time: String,
}

#[derive(Debug, Deserialize)]
struct ShadowHostConfig {
    processes: Vec<ShadowProcessConfig>,
}

#[derive(Debug, Deserialize)]
struct ShadowConfig {
    general: ShadowGeneralConfig,
    hosts: HashMap<String, ShadowHostConfig>,
}

fn build_bootstrap_table(path: &str) -> Result<(f64, BootstrapTable), Box<dyn Error>> {
    let shadow_config: ShadowConfig =
        serde_yaml::from_reader(File::open(format!("{}/shadow.config.yaml", path))?)?;

    let bootstrap_end_time =
        if let Some(bootstrap_end_time) = shadow_config.general.bootstrap_end_time {
            parse_time(&bootstrap_end_time)?
        } else {
            0.0
        };
    let filter_time = bootstrap_end_time + EXPERIMENT_START;

    let mut ret = HashMap::new();
    for (host_name, host_config) in shadow_config.hosts {
        for proc in host_config.processes {
            if !proc.path.ends_with("mgen-client") && !proc.path.ends_with("mgen-peer") {
                continue;
            }
            let split_args: Vec<_> = match proc.args {
                ShadowProcessArgs::List(commands) => commands,
                ShadowProcessArgs::Command(command) => command
                    .split_ascii_whitespace()
                    .map(|s| s.to_string())
                    .collect(),
            };
            let configs_in_args = split_args
                .into_iter()
                .filter_map(|arg| {
                    if arg.contains(".yaml") {
                        let glob_string =
                            format!("{}/shadow.data/hosts/{}/{}", path, host_name, arg,);
                        Some(glob::glob(&glob_string).expect(&glob_string))
                    } else {
                        None
                    }
                })
                .flatten();
            for config in configs_in_args {
                let config: UserConfig = serde_yaml::from_reader(File::open(config?)?)?;
                for conversation in config.conversations {
                    let key = (config.user.clone(), conversation.group);
                    let bootstrap = EXPERIMENT_START
                        + parse_time(&proc.start_time)?
                        + conversation.bootstrap.unwrap_or(config.bootstrap);
                    ret.insert(key, bootstrap);
                }
            }
        }
    }
    Ok((filter_time, ret))
}

fn core(path: Option<String>) -> Result<(), Box<dyn Error>> {
    let path = path.unwrap_or(".".to_string());

    let (_filter_time, bootstrap_table) = build_bootstrap_table(&path)?;
    // we actually don't set the full bootstrap as bootstrap, so we need to set this manually
    let filter_time = EXPERIMENT_START + 20.0 * 60.0;

    let rtt_all_file = File::create("rtt_all.mgen.json")?;
    let rtt_timeout_file = File::create("rtt_timeout.mgen.json")?;
    let rtt_mean_file = File::create("rtt_mean.mgen.json")?;
    let sent_count_file = File::create("counts.mgen.json")?;
    let timeout_by_send_file = File::create("timeout_by_send.mgen.json")?;
    let timeout_by_receive_file = File::create("timeout_by_receive.mgen.json")?;

    let mut rtt_all_ser = serde_json::Serializer::new(rtt_all_file);
    let mut rtt_timeout_ser = serde_json::Serializer::new(rtt_timeout_file);
    let mut rtt_mean_ser = serde_json::Serializer::new(rtt_mean_file);
    let mut sent_count_ser = serde_json::Serializer::new(sent_count_file);
    let mut timeout_by_send_ser = serde_json::Serializer::new(timeout_by_send_file);
    let mut timeout_by_receive_ser = serde_json::Serializer::new(timeout_by_receive_file);

    let rtt_all = rtt_all_ser.serialize_seq(None)?;
    let rtt_timeout = rtt_timeout_ser.serialize_seq(None)?;
    let rtt_mean = rtt_mean_ser.serialize_seq(None)?;
    let sent_count = sent_count_ser.serialize_seq(None)?;
    let timeout_by_send = timeout_by_send_ser.serialize_seq(None)?;
    let timeout_by_receive = timeout_by_receive_ser.serialize_seq(None)?;

    let mut serializers = Serializers {
        rtt_all,
        rtt_timeout,
        rtt_mean,
        sent_count,
        timeout_by_send,
        timeout_by_receive,
    };

    let logs = glob::glob(&format!(
        "{}/shadow.data/hosts/*client*/mgen-*.stdout",
        path
    ))?;
    for log in logs {
        process_log(&log?, filter_time, &bootstrap_table, &mut serializers)?;
    }
    serializers.rtt_all.end()?;
    serializers.rtt_timeout.end()?;
    serializers.rtt_mean.end()?;
    serializers.sent_count.end()?;
    serializers.timeout_by_send.end()?;
    serializers.timeout_by_receive.end()?;
    Ok(())
}

fn main() {
    let mut args = std::env::args();
    let _ = args.next();
    let path = args.next();
    if let Err(err) = core(path) {
        println!("error running core: {}", err);
        process::exit(1);
    }
}
