use itertools::Itertools;
use serde::{Deserialize, Deserializer};
use serde_repr::Deserialize_repr;
use std::cmp::min;
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::str::FromStr;
use time::{Duration, OffsetDateTime as Time};

pub type UserId = i32;

fn deserialize_timestamp<'de, D>(d: D) -> Result<Time, D::Error>
where
    D: Deserializer<'de>,
{
    let timestamp = i128::deserialize(d)? * 1_000_000;
    Ok(Time::from_unix_timestamp_nanos(timestamp).unwrap())
}

fn deserialize_messages<'de, D>(d: D) -> Result<Vec<Message>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(BTreeMap::<u32, Message>::deserialize(d)?
        .into_values()
        .collect())
}

fn deserialize_user_count<'de, D>(d: D) -> Result<usize, D::Error>
where
    D: Deserializer<'de>,
{
    let user_count = f32::deserialize(d)?;
    Ok(user_count.trunc() as usize)
}

#[derive(Deserialize_repr, Debug)]
#[repr(u8)]
enum MessageType {
    Text = 1,
    Media = 2,
    Image = 3,
    Audio = 4,
    Video = 5,
    Location = 6,
    Contact = 7,
    Document = 8,
    Gif = 9,
    Sys = 10,
}

#[derive(Deserialize, Debug)]
pub struct Message {
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub date: Time,
    pub user: UserId,
    //message_type: MessageType,
    //message_hash: i32,
    pub char_count: i32,
    pub emoji_count: u32,
}

#[derive(Deserialize, Debug)]
pub struct Conversation {
    pub hash: i32,
    //mail_hash: i32,
    //mail_title_hash: i32,
    //#[serde(deserialize_with = "deserialize_timestamp")]
    //date_receive: Time,
    //#[serde(deserialize_with = "deserialize_timestamp")]
    //date_first_message: Time,
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub date_last_message: Time,
    #[serde(deserialize_with = "deserialize_user_count")]
    pub user_count: usize,
    //message_count: u32,
    #[serde(deserialize_with = "deserialize_messages")]
    pub messages: Vec<Message>,
}

pub fn format_list<I, D>(iterable: I) -> String
where
    I: IntoIterator<Item = D>,
    D: std::fmt::Display,
{
    let body = iterable.into_iter().join(",");
    body + "\n"
}

/// A histogram of messages per minute, separated by an hour of 0s on both ends,
/// plus data to uniquely identify the run.
pub struct DataRun {
    pub conversation_id: i32,
    pub first_message: usize,
    pub minute_counters: Vec<u16>,
}

/// All of the runs associated with the user.
pub struct UserStats {
    pub user: UserId,
    pub data_runs: Vec<DataRun>,
}

impl UserStats {
    pub fn log_counters(self, path: &str) {
        let lens = self.data_runs.iter().map(|l| l.minute_counters.len());
        let lens_str = format_list(lens);

        let convo_ids = self.data_runs.iter().map(|l| l.conversation_id);
        let convos_str = format_list(convo_ids);

        let first_messages = self.data_runs.iter().map(|l| l.first_message);
        let first_messages_str = format_list(first_messages);

        let counters = self.data_runs.into_iter().flat_map(|l| l.minute_counters);
        let counters_str = format_list(counters);

        let path_str = format!("{}/{}.dat", path, self.user);
        let full_path = std::path::Path::new(&path_str);
        let mut file = match std::fs::File::create(full_path) {
            Ok(file) => file,
            Err(e) => panic!("Failed to open {}: {}", path_str, e),
        };

        file.write_all(counters_str.as_bytes())
            .unwrap_or_else(|e| panic!("Failed to write to {}: {}", path_str, e));
        file.write_all(lens_str.as_bytes())
            .unwrap_or_else(|e| panic!("Failed to write to {}: {}", path_str, e));
        file.write_all(convos_str.as_bytes())
            .unwrap_or_else(|e| panic!("Failed to write to {}: {}", path_str, e));
        file.write_all(first_messages_str.as_bytes())
            .unwrap_or_else(|e| panic!("Failed to write to {}: {}", path_str, e));
    }
}

pub fn process_conversation(conversation: Conversation) -> Vec<UserStats> {
    struct ProcStats {
        start: Time,
        last: Time,
        data_runs: Vec<DataRun>,
    }

    let mut convo_users: HashMap<UserId, ProcStats> =
        HashMap::with_capacity(conversation.user_count);

    for (i, message) in conversation.messages.iter().enumerate() {
        let stats = convo_users.entry(message.user).or_insert(ProcStats {
            start: message.date - Duration::HOUR,
            last: message.date,
            data_runs: vec![DataRun {
                conversation_id: conversation.hash,
                first_message: i,
                minute_counters: vec![0; 60],
            }],
        });
        stats.last = message.date;
        let mut message_minute = (message.date - stats.start).whole_minutes() as usize;
        let last_data_run = stats.data_runs.last_mut().unwrap();
        let data_run = if last_data_run.minute_counters.len() + 120 < message_minute {
            // last message was sent at least two hours ago,
            // add an hour of no messages to the end of the last counts
            last_data_run.minute_counters.append(&mut vec![0; 60]);
            // and start a new set of counts (0-filling an hour before)
            stats.start = message.date - Duration::HOUR;
            message_minute = 60;
            let data_run = DataRun {
                conversation_id: conversation.hash,
                first_message: i,
                minute_counters: vec![0; 60],
            };
            stats.data_runs.push(data_run);
            stats.data_runs.last_mut().unwrap()
        } else {
            // last message was sent less than two hours ago,
            // continue using the existing counts
            last_data_run
        };

        if message_minute >= data_run.minute_counters.len() {
            let to_fill = 1 + message_minute - data_run.minute_counters.len();
            data_run.minute_counters.append(&mut vec![0; to_fill]);
        }

        data_run.minute_counters[message_minute] += 1;
    }
    convo_users
        .into_iter()
        .map(|(user, stats)| {
            let mut data_runs = stats.data_runs;
            let last_run = data_runs.last_mut().unwrap();
            // 0-fill another hour or up to the end of the conversation
            let to_fill =
                min(stats.last + Duration::HOUR, conversation.date_last_message) - stats.last;
            last_run
                .minute_counters
                .append(&mut vec![0; to_fill.whole_minutes() as usize]);

            UserStats { user, data_runs }
        })
        .collect()
}

pub fn create_weighted<I>(values: I) -> (Vec<usize>, Vec<I::Item>)
where
    I: std::iter::IntoIterator,
    I::Item: std::cmp::Eq + std::hash::Hash + Ord,
{
    let counter = values.into_iter().collect::<counter::Counter<_>>();
    let mut collected = counter.into_iter().collect::<Vec<_>>();
    collected.sort();
    let (items, counts): (Vec<_>, Vec<_>) = collected.into_iter().unzip();
    (counts, items)
}

pub fn write_weighted<I>(values: I, file_path: &str) -> std::io::Result<()>
where
    I: std::iter::IntoIterator,
    I::Item: std::cmp::Eq + std::hash::Hash + std::fmt::Display + Ord,
{
    let (counts, vals) = create_weighted(values);
    let counts = format_list(counts);
    let vals = format_list(vals);
    let data = format!("{}{}", counts, vals);

    std::fs::write(file_path, data.as_bytes())
}

use rand_distr::WeightedAliasIndex;

pub fn parse_weights_file<T>(path: String) -> anyhow::Result<(WeightedAliasIndex<u32>, Vec<T>)>
where
    T: FromStr,
    <T as FromStr>::Err: std::error::Error,
{
    let weights_file = std::fs::read_to_string(path)?;
    let mut weights_lines = weights_file.lines();
    let weights = weights_lines
        .next()
        .unwrap()
        .split(',')
        .map(u32::from_str)
        .collect::<Result<Vec<_>, _>>()?;
    let vals = weights_lines
        .next()
        .expect("Weights file only has one line")
        .split(',')
        .map(T::from_str)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        weights.len() == vals.len(),
        "Weights file doesn't have the same number of weights and values."
    );
    let dist = WeightedAliasIndex::<u32>::new(weights)?;
    Ok((dist, vals))
}
