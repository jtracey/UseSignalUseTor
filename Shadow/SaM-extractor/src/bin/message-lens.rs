use rand_distr::Distribution;
use rayon::prelude::*;
use sam_extractor::*;
use std::collections::HashMap;

fn bytes_to_blocks(bytes: i32) -> usize {
    if bytes <= 112 {
        1
    } else {
        ((bytes + 208) / 160) as usize
    }
}

fn main() {
    let mut args = std::env::args();
    let this_program = args.next().unwrap();

    if args.len() < 2 {
        panic!(
            "Usage: {} [-s file_sizes.dat] stats_directory chat.json...",
            this_program
        );
    }

    let first_arg = args.next().unwrap();
    let (file_sizes, dists_dir) = if first_arg != "-s" {
        (None, first_arg)
    } else {
        (
            Some(parse_weights_file(args.next().unwrap()).unwrap()),
            args.next().unwrap(),
        )
    };

    let conversations = args
        .flat_map(|a| glob::glob(a.as_str()).unwrap())
        .collect::<Vec<_>>()
        .into_par_iter()
        .map(|file| {
            let file = file.unwrap();
            let data = std::fs::read_to_string(file.clone()).expect("Unable to read file");
            serde_json::from_str::<Conversation>(&data)
                .unwrap_or_else(|e| panic!("Unable to parse {:?}: {:?}", &file, e))
        })
        .collect::<Vec<_>>();

    let mut users: HashMap<UserId, Vec<usize>> = HashMap::new();
    let mut rng = rand::thread_rng();
    for conversation in conversations {
        for message in conversation.messages {
            let file_size = if let Some((ref dist, ref sizes)) = file_sizes {
                sizes[dist.sample(&mut rng)]
            } else {
                0
            };
            let message_len =
                bytes_to_blocks(message.char_count + message.emoji_count as i32 * 4 + file_size);
            if let Some(lens) = users.get_mut(&message.user) {
                lens.push(message_len);
            } else {
                users.insert(message.user, vec![message_len]);
            }
        }
    }

    for (user, sizes) in users {
        let dists_dir_str = format!("{}/{}/", dists_dir, user);
        if std::path::Path::new(&dists_dir_str)
            .try_exists()
            .expect("failed to check path existence")
        {
            write_weighted(sizes, &format!("{}sizes.dat", &dists_dir_str))
                .expect("failed to write data");
        }
    }
}
