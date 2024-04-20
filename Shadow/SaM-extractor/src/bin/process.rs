use rayon::prelude::*;
use sam_extractor::*;
use std::collections::HashMap;
use time::Duration;

const HOUR_AS_SECONDS: i64 = 60 * 60;

#[derive(PartialEq)]
enum Direction {
    Sent,
    Received,
}

#[derive(Clone, Copy, PartialEq)]
enum State {
    Idle,
    Active,
}

struct LabeledMessage {
    state: State,
    /// True iff last sent message is from an ongoing Active state
    continuing: bool,
    last_message: Direction,
    /// True iff last message is not from an ongoing Active state
    idle_sent: bool,
    iit: Duration,
    sent_iit: Option<Duration>,
}

/// Same as the UserStats struct, extended with the state guesses
struct Stats {
    user_stats: UserStats,
    states: Vec<State>,
}

fn parse_stats_file(user: UserId, data: String) -> Stats {
    let mut lines = data.lines();
    let concat_minute_counters = lines
        .next()
        .unwrap()
        .split(',')
        .map(|s| s.parse::<u16>().unwrap())
        .collect::<Vec<_>>();
    let lens = lines
        .next()
        .unwrap()
        .split(',')
        .map(|s| s.parse::<usize>().unwrap());
    let mut convos = lines.next().unwrap().split(',').map(|s| s.parse().unwrap());
    let mut first_messages = lines.next().unwrap().split(',').map(|s| s.parse().unwrap());
    let states = lines
        .next()
        .unwrap()
        .split(',')
        .map(|s| if s == "0" { State::Idle } else { State::Active })
        .collect();

    let mut data_runs: Vec<DataRun> = vec![];
    let mut index = 0;
    for len in lens {
        let minute_counters = concat_minute_counters[index..index + len].to_vec();
        index += len;
        let conversation_id: i32 = convos.next().unwrap();
        let first_message: usize = first_messages.next().unwrap();
        let data_run = DataRun {
            conversation_id,
            first_message,
            minute_counters,
        };
        data_runs.push(data_run);
    }

    let user_stats = UserStats { user, data_runs };
    Stats { user_stats, states }
}

/// Returns Idle iff the message at the index is labeled by the HMM data as Idle,
/// or if all other sent messages are separated by at least one idle period
fn determine_state(counters: &[u16], states: &[State], idx: usize) -> State {
    if states[idx] == State::Idle {
        return State::Idle;
    }

    if counters[idx] > 1 {
        return State::Active;
    }

    for i in (0..idx).rev() {
        if states[i] == State::Idle {
            return State::Idle;
        }
        if counters[i] > 0 {
            break;
        }
    }

    for i in (idx + 1)..states.len() {
        if states[i] == State::Idle {
            return State::Idle;
        }
        if counters[i] > 0 {
            return State::Active;
        }
    }
    State::Idle
}

/// Returns True iff the two messages are NOT part of the same Active state
fn different_run(states: &[State], idx1: usize, idx2: usize) -> bool {
    states[idx1..idx2].iter().any(|s| s == &State::Idle)
}

/// Returns True iff the last message is not part of an ongoing Active state
fn is_transition(states: &[State], idx: usize, last_msg_minute: i64) -> bool {
    if states[idx] == State::Idle || last_msg_minute < 0 {
        return true;
    }

    let prev_idx = last_msg_minute as usize;
    different_run(states, prev_idx, idx)
}

/// Takes a Stats struct, and a Vec of Conversations the user is in,
/// returns a Vec of the labeled messages and a count of messages received while idle.
fn stats_to_labeled(
    stats: Stats,
    conversations: HashMap<i32, &Conversation>,
) -> (Vec<LabeledMessage>, usize) {
    let user = stats.user_stats.user;

    let mut labeled_messages = vec![];
    let mut received_messages = conversations
        .values()
        .flat_map(|c| c.messages.iter().filter(|m| m.user != user))
        .count();

    let mut i = 0;
    for data_run in stats.user_stats.data_runs {
        let minutes = data_run.minute_counters;
        let next_i = minutes.len();

        // get rid of the fake leading and trailing 0s
        let (zeros, minutes) = minutes.split_at(60);
        assert_eq!(zeros, [0; 60]);
        let lower = i + 60;
        let upper = lower + minutes.len() - minutes.iter().rev().position(|c| c > &0).unwrap();
        // FIXME: we can remove this once we've run tests confirming no off-by-ones
        let (minutes, zeros) = minutes.split_at(upper - lower);
        assert_eq!(
            zeros,
            &[0; 60][0..zeros.len()],
            "zeros is not zero'd: {:?}",
            zeros
        );
        let states = &stats.states[lower..upper];

        let conversation = conversations
            .get(&data_run.conversation_id)
            .unwrap_or_else(|| {
                panic!(
                    "conversation {} not found for user {}",
                    data_run.conversation_id, user
                )
            });

        let first_msg_date = conversation.messages[data_run.first_message].date;
        let final_msg_date = first_msg_date + Duration::minutes(states.len() as i64);
        let mut prev_sent_time = None;
        let mut prev_sent_minute_idx = None;
        for msg_i in data_run.first_message..conversation.messages.len() {
            if msg_i == 0 {
                continue;
            }
            let msg = &conversation.messages[msg_i];
            if msg.date > final_msg_date {
                break;
            }
            let minute_idx = (msg.date - first_msg_date).whole_minutes() as usize;
            if msg.user == user {
                let state = determine_state(minutes, states, minute_idx);

                let last_msg = &conversation.messages[msg_i - 1];
                //let last_msg_state = determine_state(minutes, states, last_msg_minute_idx);
                let last_message = if last_msg.user == user {
                    Direction::Sent
                } else {
                    Direction::Received
                };
                let last_msg_minute = (last_msg.date - first_msg_date).whole_minutes();
                let transition = is_transition(states, minute_idx, last_msg_minute);
                let iit = msg.date - last_msg.date;

                let continuing = if let Some(idx2) = prev_sent_minute_idx {
                    !different_run(states, idx2, minute_idx)
                } else {
                    false
                };

                let sent_iit = prev_sent_time.map(|date| msg.date - date);

                let labeled_message = LabeledMessage {
                    state,
                    continuing,
                    last_message,
                    idle_sent: transition,
                    iit,
                    sent_iit,
                };
                labeled_messages.push(labeled_message);

                prev_sent_time = Some(msg.date);
                prev_sent_minute_idx = Some(minute_idx);
            } else if minute_idx < states.len() && states[minute_idx] == State::Active {
                received_messages -= 1;
            }
        }

        i = next_i;
    }

    (labeled_messages, received_messages)
}

/// Returns a list of IITs between a transition message and the previous sent message.
fn idle_iits(messages: &[LabeledMessage]) -> Vec<i64> {
    messages
        .iter()
        .filter_map(|m| {
            if m.idle_sent {
                m.sent_iit.map(|t| t.whole_seconds())
            } else {
                None
            }
        })
        .collect()
}

/// For each stretch of sent Active messages, returns the max IIT in that Active stretch.
fn active_iits(messages: &[LabeledMessage]) -> Vec<i64> {
    let mut max_iit = Duration::ZERO;
    let mut active = false;
    let mut ret = vec![];
    for message in messages {
        if message.continuing {
            max_iit = std::cmp::max(max_iit, message.iit);
            active = true;
        } else if active {
            active = false;
            if max_iit != Duration::ZERO {
                ret.push(max_iit.whole_seconds());
            }
            max_iit = Duration::ZERO;
        }
    }
    ret
}

/// Returns a list of IITs between any active sent message and the given active message.
fn sent_sent_iits(messages: &[LabeledMessage]) -> Vec<i64> {
    messages
        .iter()
        .filter_map(|m| {
            if !m.idle_sent && m.last_message == Direction::Sent {
                Some(m.iit.whole_seconds())
            } else {
                None
            }
        })
        .collect()
}

/// Returns a list of IITs between any active received message and the given active message.
fn received_sent_iits(messages: &[LabeledMessage]) -> Vec<i64> {
    messages
        .iter()
        .filter_map(|m| {
            if !m.idle_sent && m.last_message == Direction::Received {
                Some(m.iit.whole_seconds())
            } else {
                None
            }
        })
        .collect()
}

/// Returns the fraction of idle sent messages with active transitions.
fn send_transition_frac(messages: &[LabeledMessage]) -> f64 {
    let idle_sent = messages.iter().filter(|m| m.idle_sent).count() as f64;
    let transition = messages
        .iter()
        .filter(|m| m.idle_sent && m.state == State::Active)
        .count() as f64;
    transition / idle_sent
}

/// Returns the count of idle received messages with active transitions.
fn receive_transition_count(messages: &[LabeledMessage]) -> usize {
    // bit of a hack, we're actually counting sent messages that are active,
    // but aren't coming from an idle state,
    // and where the previous message wasn't from this active state,
    // since that only leaves the possible case of an active transition from receiving a message,
    // and any received messages that didn't cause a message to be sent can't possibly have
    // transitioned to idle in the HMM
    messages
        .iter()
        .filter(|m| !m.idle_sent && !m.continuing && m.state == State::Active)
        .count()
}

fn main() {
    let mut args = std::env::args();
    let this_program = args.next().unwrap();

    if args.len() < 3 {
        panic!("Usage: {} dists_dir stats_dir chat.json...", this_program);
    }

    let dists_dir = args.next().unwrap();
    let stats_dir = args.next().unwrap();

    let conversations: Vec<_> = args
        .flat_map(|a| glob::glob(a.as_str()).unwrap())
        .collect::<Vec<_>>()
        .into_par_iter()
        .map(|file| {
            let file = file.unwrap();
            let data = std::fs::read_to_string(file.clone()).expect("Unable to read file");
            serde_json::from_str::<Conversation>(&data)
                .unwrap_or_else(|e| panic!("Unable to parse {:?}: {:?}", &file, e))
        })
        .collect();

    let mut user_convo: HashMap<UserId, HashMap<i32, &Conversation>> = HashMap::new();
    for conversation in conversations.iter() {
        for user in conversation.messages.iter().map(|m| m.user) {
            user_convo
                .entry(user)
                .or_default()
                .insert(conversation.hash, conversation);
        }
    }

    user_convo
        .into_par_iter()
        .for_each(|(user, conversations)| {
            let stats_path_str = format!("{}/{}.dat", stats_dir, user);
            let stats_file = std::path::Path::new(&stats_path_str);
            let data = if let Ok(data) = std::fs::read_to_string(stats_file) {
                data
            } else {
                println!("failed to read stats file: {}", stats_path_str);
                return;
            };
            //println!("{}", user);

            let stats = parse_stats_file(user, data);
            let (labeled, idle_received) = stats_to_labeled(stats, conversations);

            let mut real_data = false;

            // all IITs for idle messages
            let idle_dist = idle_iits(&labeled);
            let idle_dist = if !idle_dist.is_empty() {
                real_data = true;
                idle_dist
            } else {
                // User sent ~no messages while idle,
                // so simulated should always idle for an hour.
                vec![HOUR_AS_SECONDS]
            };

            // max IIT for string of active messages
            let active_dist = active_iits(&labeled);
            let active_dist = if !active_dist.is_empty() {
                real_data = true;
                active_dist
            } else {
                // No stretches of active sent messages exist,
                // so the user always immediately transitions to idle,
                // so use a single time of 0.
                vec![0]
            };

            // IITs for sent-sent active messages
            let a_s_dist = sent_sent_iits(&labeled);
            let a_s_dist = if !a_s_dist.is_empty() {
                real_data = true;
                a_s_dist
            } else {
                // User never sent two messages in a row while active,
                // so simulated should always wait an hour before sending.
                vec![HOUR_AS_SECONDS]
            };

            // IITs for received-sent active messages
            let a_r_dist = received_sent_iits(&labeled);
            let a_r_dist = if !a_r_dist.is_empty() {
                real_data = true;
                a_r_dist
            } else {
                // User never replied to a received message while active,
                // so simulated should always wait an hour before sending.
                vec![HOUR_AS_SECONDS]
            };

            // fraction of idle sent messages with active transitions
            let s_prob = send_transition_frac(&labeled);
            let s_prob = if s_prob.is_finite() && s_prob != 0.0 {
                // we might want to make this contingent on s_prob != 1.0,
                // but that would imply there were further active messages
                // (because a message is only active if multiple messages
                // got sent while active in the same stretch),
                // so it should be fine regardless
                real_data = true;
                s_prob
            } else {
                // If the user never sent any idle messages,
                // this value *should* be undefined,
                // but because the associated distribution for sending idle
                // messages should be set to "infinity" (one hour) anyway,
                // making this unused in practice,
                // and because we don't want to force the consumer to handle non-finite floats,
                // we instead use 0.
                0.0
            };

            // fraction of idle received messages with active transitions
            let r_count = receive_transition_count(&labeled);
            let r_prob = if idle_received != 0 && r_count != idle_received {
                if r_count != 0 {
                    real_data = true;
                    // otherwise, this user never transitioned to active from receiving,
                    // so we better have some other data
                }
                r_count as f64 / idle_received as f64
            } else {
                // user never received any messages while idle, or always transitioned;
                // we'll just always transition to active and let that state handle it;
                // either way, that's not enough data to consider this valid on its own
                1.0
            };

            if !real_data {
                println!("{} has no data", user);
                return;
            }

            let dists_dir_str = format!("{}/{}/", dists_dir, user);
            std::fs::create_dir_all(&dists_dir_str).expect("unable to create dists directory");

            write_weighted(idle_dist, &format!("{}I.dat", &dists_dir_str))
                .expect("failed to write data");
            write_weighted(active_dist, &format!("{}W.dat", &dists_dir_str))
                .expect("failed to write data");
            write_weighted(a_s_dist, &format!("{}As.dat", &dists_dir_str))
                .expect("failed to write data");
            write_weighted(a_r_dist, &format!("{}Ar.dat", &dists_dir_str))
                .expect("failed to write data");

            std::fs::write(
                format!("{}S.dat", &dists_dir_str),
                s_prob.to_string().as_bytes(),
            )
            .expect("failed to write data");
            std::fs::write(
                format!("{}R.dat", &dists_dir_str),
                r_prob.to_string().as_bytes(),
            )
            .expect("failed to write data");
        });
}
