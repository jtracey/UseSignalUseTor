#!/usr/bin/env python3

import argparse
import glob
import os
import random
import shutil

from yaml import load, dump
try:
    from yaml import CLoader as Loader, CDumper as Dumper
except ImportError:
    from yaml import Loader, Dumper

SECONDS_IN_HOUR = 60.0 * 60.0

class Conversation:
    def __init__(self, size, users, waits):
        self.size = size
        self.users = users
        self.waits = waits

    def merge(self, other):
        self.users.extend(other.users)
        self.waits.extend(other.waits)
        return

    def merge_slice(conversations):
        first = conversations.pop()
        return [first.merge(o) for o in conversations]

    def add_silent_members(self, users):
        self.users.extend(users)
        self.waits.extend([SECONDS_IN_HOUR] * len(users))

class User:
    def __init__(self, name, dists_path, client, tor_process):
        self.name = name
        self.dists_path = dists_path
        self.client = client
        self.tor_process = tor_process
        self.conversations = []

    def socks_port(self):
        # default tor socks port is 9050, default tor control port is 9051
        # each additional process needs both of those, so socks port goes up by 2
        return 9050 + self.tor_process * 2

    def control_port(self):
        return self.socks_port() + 1

    def save(self, config):
        assert(config['hosts'] is not None)
        client_path = '~/.cargo/bin/mgen-client'
        mgen_config_path = self.client + self.name + '.yaml'
        host_name = self.client.split('/')[-2]
        print("saving: ", self.name, flush=True)
        host = config['hosts'][host_name]
        process = next((p for p in host['processes'] if p['path'] == client_path), None)

        tors = [p for p in host['processes'] if p['path'] == '~/.local/bin/tor']
        torrc = '{}.torrc'.format(self.tor_process)
        tor_datadir = "tor-{}".format(self.tor_process)
        tor_start = tors[0]['start_time']
        if process is None:
            if len(tors) == 0:
                print('Error: No tor process for client {} in shadow config.'.format(self.client))
                exit(1)
            proc = {
                'path': client_path,
                'args': 'user*.yaml',
                'start_time': 1170, #tor_start + 60,
                'expected_final_state': 'running'
            }
            host['processes'].append(proc)
        if self.tor_process != 0 and not any('-f {}'.format(torrc) in tor['args'] for tor in tors):
            # we haven't setup this tor client yet, handle that first
            tor_proc = {
                'path': tors[0]['path'],
                'args': '--defaults-torrc torrc-defaults -f {} --DataDirectory ./{}'.format(torrc, tor_datadir),
                'start_time': tor_start + self.tor_process,
                'expected_final_state': 'running',
                'environment': {'OPENBLAS_NUM_THREADS': '1'}
            }
            host['processes'].append(tor_proc)
            torrc_path = self.client + torrc
            torrc_contents = "SocksPort {}\n".format(self.socks_port())
            torrc_contents += "ControlPort {}\n".format(self.control_port())
            with open(torrc_path, 'w') as f:
                f.write(torrc_contents)
            os.mkdir(self.client + tor_datadir)

        yaml_str = 'user: "{}"\n'.format(self.name)
        # proxy starts commented out for baseline testing,
        # a simple sed replacement can enable it
        yaml_str += '#socks: "127.0.0.1:{}"\n'.format(self.socks_port())
        # defaults
        yaml_str += 'message_server: "1.1.1.2:6397"\n'
        yaml_str += 'web_server: "1.1.1.3:6398"\n'
        yaml_str += 'bootstrap: 5.0\n'
        yaml_str += 'retry: 5.0\n'
        yaml_str += 'distributions:\n'
        with open(self.dists_path + '/S.dat') as f:
            s = f.read().strip()
        yaml_str += '  s: {}\n'.format(s)
        with open(self.dists_path + '/R.dat') as f:
            r = f.read().strip()
        yaml_str += '  r: {}\n'.format(r)
        weighted_format = '  {}: {{ distribution: "Weighted", weights_file: "' + self.dists_path + '/{}.dat" }}\n'
        yaml_str += weighted_format.format('m', 'sizes')
        yaml_str += weighted_format.format('i', 'I')
        yaml_str += weighted_format.format('w', 'W')
        yaml_str += weighted_format.format('a_s', 'As')
        yaml_str += weighted_format.format('a_r', 'Ar')
        yaml_str += 'conversations:\n'
        for group in self.conversations:
            yaml_str += '  - group: "{}"\n'.format(group[0].name)
            yaml_str += '    bootstrap: {}\n'.format(group[1])
            yaml_str += '    message_server: "{}:6397"\n'.format(group[0].server_ip)
            yaml_str += '    web_server: "{}:6398"\n'.format(group[0].web_ip)
        with open(mgen_config_path, 'w') as f:
            f.write(yaml_str)

def normalize_weights(weights):
    """ Normalize weights so they sum to 1 """
    tot = sum(weights)
    return [w/tot for w in weights]

def read_dist_file(path):
    with open(path) as f:
        (weights, vals) = f.readlines()
        vals = list(map(int, vals.split(',')))
        weights = normalize_weights(list(map(float, weights.split(','))))
        return vals, weights

def read_dist_file_float(path):
    with open(path) as f:
        (weights, vals) = f.readlines()
        vals = list(map(float, vals.split(',')))
        weights = normalize_weights(list(map(float, weights.split(','))))
        return vals, weights

def main():
    parser = argparse.ArgumentParser(
        description="Generate messenger clients for use with mgen and shadow.")
    parser.add_argument('--dyadic', type=str, help='File containging the weighted distribution of the number of dyadic (1-on-1) conversations a user may have.', required=True)
    parser.add_argument('--group', type=str, help='File containging the weighted distribution of the number of group conversations a user may have.', required=True)
    parser.add_argument('--participants', type=str, help='File containing the weighted distribution of the number of participants in a group conversation.', required=True)
    parser.add_argument('--config', type=str, help='The original shadow.config.yaml file; a modified copy will be placed in the same directory as mnet.shadow.config.yaml', required=True)
    parser.add_argument('--clients', type=str, help='Glob specifying the paths to shadow host template directories where users will be assigned uniformly at random. This will also determine where the server and web server directories are created.', required=True)
    parser.add_argument('--empirical', type=str, help='Path of directory containing the directories for each empirical user distribution data.', required=True)
    parser.add_argument('--users', type=int, help='Number of concurrent simulated users to generate.', required=True)
    parser.add_argument('--servers', type=int, default=1, help='Number of message and web servers to run (defaults to 1).')
    parser.add_argument('--tors', type=int, default=0, help='Number of additional tor processes to run (if 0 or unset, clients use the original tor process, else clients only use new processes).')
    parser.add_argument('--seed', type=int, help='RNG seed, if deterministic config generation is desired.')
    args = parser.parse_args()

    random.seed(args.seed, version=2)

    print("loading config...", flush=True)
    with open(args.config) as f:
        config = load(f, Loader=Loader)
    assert(config['hosts'] is not None)

    print("adding servers to config...", flush=True)
    taken_ips = {host['ip_addr'] for host in config['hosts'].values() if 'ip_addr' in host}
    servers = []
    webs = []
    server_start = 300
    server_idle = 900
    # bump the bootstrap time to account for our bootstrap as well
    config['general']['bootstrap_end_time'] = server_start + server_idle
    for server_num in range(args.servers):
        server_ip = get_free_ip(2, taken_ips)
        servers.append(server_ip)
        web_ip = get_free_ip(int(server_ip.split('.')[-1]) + 1, taken_ips)
        webs.append(web_ip)
        taken_ips |= {server_ip, web_ip}

        server_name = 'server{}'.format(server_num)
        web_name = 'web{}'.format(server_num)

        config['hosts'][server_name] = {
            'network_node_id': 2521, # FIXME: is this sufficiently stable?
            'ip_addr': server_ip,
            'bandwidth_down': '10000000 kilobit',
            'bandwidth_up': '10000000 kilobit',
            'processes': [{
                'path': '~/.cargo/bin/mgen-server',
                'args': ['server.crt', 'server.key', '{}:6397'.format(server_ip), str(server_idle)],
                'start_time': str(server_start),
                'expected_final_state': 'running'
            }]
        }
        config['hosts'][web_name] = {
            'network_node_id': 2521, # FIXME: is this sufficiently stable?
            'ip_addr': web_ip,
            'bandwidth_down': '10000000 kilobit',
            'bandwidth_up': '10000000 kilobit',
            'processes': [{
                'path': '~/.cargo/bin/mgen-web',
                'args': ['server.crt', 'server.key', '{}:6398'.format(web_ip)],
                'start_time': str(server_start),
                'expected_final_state': 'running'
            }]
        }

    dyadic_dist_vals, dyadic_dist_weights = read_dist_file(args.dyadic)
    group_dist_vals, group_dist_weights = read_dist_file(args.group)
    participants_dist_vals, participants_dist_weights = read_dist_file(args.participants)

    client_paths = glob.glob(args.clients)
    empirical_users = [args.empirical + '/' + f for f in os.listdir(args.empirical)]

    print("caching idle distributions...", flush=True)
    idles = { path: read_dist_file_float(path + '/I.dat') for path in empirical_users }

    conversations = {2: []}
    users = set()
    print("sampling users...", flush=True)
    for i in range(args.users):
        user = sample_user(i, empirical_users, client_paths, args.tors)

        num_dyadic = sample_dyadic_conversation_count(dyadic_dist_vals, dyadic_dist_weights)
        num_group_conversations = sample_group_conversation_count(group_dist_vals, group_dist_weights)

        idle_dist_vals, idle_dist_weights = idles[user.dists_path]
        initial_waits = sample_initial_idle(idle_dist_vals, idle_dist_weights, num_dyadic + num_group_conversations)

        conversations[2].extend([Conversation(2, [user], [initial_waits.pop()]) for _ in range(num_dyadic)])
        for c in range(num_group_conversations):
            num_participants = sample_participant_count(participants_dist_vals, participants_dist_weights)
            if num_participants not in conversations:
                conversations[num_participants] = []
            conversations[num_participants].append(Conversation(num_participants, [user], [initial_waits.pop()]))
        users.add(user)

    group_count = 0
    for size in sorted(conversations):
        print("creating groups of size {}...".format(size), flush=True)
        remaining = conversations[size]
        grouped = []
        group = Conversation(size, [], [])
        while len(remaining) > 0:
            if len(group.users) == size:
                grouped.append(group)
                group = Conversation(size, [], [])
            for i in reversed(range(len(remaining))):
                if remaining[i].users[0] not in group.users:
                    group.merge(remaining.pop(i))
                    break
            else:
                # no remaining users not already in the group, we have to move on
                # (n.b. this is a python for/else, not an if/else)
                grouped.append(group)
                group = Conversation(size, [], [])
                break
        for group in grouped:
            group.name = "group" + str(group_count)
            server_num = random.randint(0, args.servers - 1)
            group.server_ip = servers[server_num]
            group.web_ip = webs[server_num]
            #print("creating group {} (size: {}, active members: {})".format(group.name, group.size, len(group.users)))
            if group.size == len(group.users):
                create_group(group)
            else:
                # add silent members to pad out group
                sample_from = list(users - set(group.users))
                sample_count = group.size - len(group.users)
                if len(sample_from) < sample_count:
                    print("Error: trying to sample {} users from {} users not already in the group; try increasing the --users count.".format(
                        sample_count, len(sample_from)))
                    exit(1)
                silent = random.sample(sample_from, sample_count)
                group.add_silent_members(silent)
                create_group(group, set(silent))
            group_count += 1

    print("saving groups to disk...", flush=True)
    for user in users:
        user.save(config)

    print("saving config...", flush=True)
    new_config = os.path.dirname(args.config) + '/mnet.shadow.config.yaml'
    with open(new_config, 'w') as f:
        dump(config, f, Dumper=Dumper)

    print("copying cert and key...", flush=True)
    cert = os.path.dirname(__file__) + '/server.crt'
    key = os.path.dirname(__file__) + '/server.key'
    split_glob = [s for s in args.clients.split('/') if s != '']
    shadow_config_path = '/'+'/'.join(split_glob[:-1])
    for server_num in range(args.servers):
        server_dir = '{}/server{}'.format(shadow_config_path, server_num)
        web_dir = '{}/web{}'.format(shadow_config_path, server_num)
        os.makedirs(server_dir, exist_ok=True)
        os.makedirs(web_dir, exist_ok=True)
        shutil.copy(cert, server_dir)
        shutil.copy(cert, web_dir)
        shutil.copy(key, server_dir)
        shutil.copy(key, web_dir)

    print("done!")

def create_group(group, silent=set()):
    if all(n >= SECONDS_IN_HOUR for n in group.waits):
        # every group member is going to do nothing, just drop it
        return
    [group.users[i].conversations.append((group, group.waits[i])) for i in range(len(group.users)) if group.users[i] not in silent]
    [user.conversations.append((group, SECONDS_IN_HOUR)) for user in silent]

def sample_user(id_number, empirical_users, client_paths, tor_processes):
    name = "user{}".format(id_number)
    dists_path = random.choice(empirical_users)
    client = random.choice(client_paths)
    tor_process = (id_number % tor_processes) + 1 if tor_processes > 0 else 0
    return User(name, dists_path, client, tor_process)

def sample_participant_count(participants_dist_vals, participants_dist_weights):
    return random.choices(participants_dist_vals, weights=participants_dist_weights)[0]

def sample_dyadic_conversation_count(dyadic_dist_vals, dyadic_dist_weights):
    return random.choices(dyadic_dist_vals, dyadic_dist_weights)[0]

def sample_group_conversation_count(group_dist_vals, group_dist_weights):
    return random.choices(group_dist_vals, group_dist_weights)[0]

# takes I distribution, the function will scale it then return a list of samples
def sample_initial_idle(idle_dist_vals, idle_dist_weights, n_samples):
    real_bootstrap = 30
    scaled_weights = [real_bootstrap + idle_dist_vals[i] * idle_dist_weights[i] for i in range(len(idle_dist_vals))]
    if sum(scaled_weights) == 0.0:
        # edge case where user always idled 0 seconds; say they were always idle instead
        return [SECONDS_IN_HOUR] * max(1, n_samples)
    return random.choices(idle_dist_vals, scaled_weights, k=n_samples)

def get_free_ip(start, taken_ips):
    for i in range(start, 256):
        ip = "1.1.1.{}".format(i)
        if ip not in taken_ips:
            return ip
    else:
        print("Error: no IPs remaining in 1.1.1.0/24, modify source to use a different unused block.")
        exit(1)

if __name__ == '__main__':
    main()
