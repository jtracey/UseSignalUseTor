use rayon::prelude::*;
use sam_extractor::*;
use std::collections::HashMap;

fn main() {
    let mut args = std::env::args();
    let this_program = args.next().unwrap();

    if args.len() < 2 {
        panic!("Usage: {} stats_directory chat.json...", this_program);
    }

    let stats_path = args.next().unwrap();

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

    let group_sizes: Vec<_> = conversations.par_iter().map(|c| c.user_count).collect();
    let mut group_size_histogram = vec![0; 256];
    for group_size in group_sizes {
        group_size_histogram[group_size] += 1;
    }

    let all_stats: Vec<_> = conversations
        .into_par_iter()
        .flat_map(process_conversation)
        .collect();
    println!("{:?}", group_size_histogram);

    let mut users: HashMap<UserId, UserStats> = HashMap::new();
    for mut stats in all_stats {
        if let Some(current_stats) = users.get_mut(&stats.user) {
            current_stats.data_runs.append(&mut stats.data_runs);
        } else {
            users.insert(stats.user, stats);
        }
    }

    //pyo3::prepare_freethreaded_python();
    let v: Vec<UserStats> = users.into_values().collect();
    v.into_par_iter()
        .for_each(|stats| stats.log_counters(&stats_path));
}
