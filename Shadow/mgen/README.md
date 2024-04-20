## MGen

MGen is a set of tools for generating simulated messenger traffic.
It is designed for use analogous to (and likely in conjunction with) [TGen](https://github.com/shadow/tgen), but for simulating traffic generated from communications in messenger apps, such as Signal or WhatsApp, rather than web traffic or file downloads.
Notably, this allows for studying network traffic properties of messenger apps in [Shadow](https://github.com/shadow/shadow).

Like TGen, MGen can create message flows built around Markov models.
Unlike TGen, these models are expressly designed with user activity in messenger clients in mind.
These messages can be relayed through a central server, which can handle group messages (i.e., traffic that originates from one sender, but gets forwarded to multiple recipients).
Alternatively, a peer-to-peer client, what we call the "peer", can be used.

Clients and peers also generate delivery receipts (small messages used to indicate to someone who sent a message that the recipient device has received it).
These receipts can make up to half of all traffic.
(Read receipts, however, are not supported.)

## Usage

MGen is written entirely in Rust, and is built like most pure Rust projects.
If you have a working [Rust install](https://rustup.rs/) with Cargo, you can build and install the peer, client, and servers with cargo by running from the project root:

`cargo install --path .`

This will generally place them somewhere in your environment's PATH.
Normal cargo features also apply—e.g., use `cargo build` to build debug builds, or use the `--release` flag to enable release optimizations without installing.
The message server can be built and executed in debug mode with `cargo run --bin mgen-server`, and similar for the client, peer, and web server (`web`).
Alternatively, you can run the executables directly from the respective target directory after building (e.g., `./target/release/server`).

### Invocation

#### Servers
`mgen-server server.crt server.key [addr:port]`

`mgen-web server.crt server.key [addr:port]`

There are two servers for the centralized model: a message server that relays messages, and a web server used to serve file attachments (no files are actually hosted or served, the traffic is simply generated).

Both servers are invoked in the same way.
The first two arguments are the paths to the server's TLS certificate and private key files.
The client is configured to skip validation, so any valid certificate with typical parameters is fine.
If you don't want to generate your own, you can find pre-generated ones in the [test server template directory](/shadow/client/shadow.data.template/hosts/server/).
The server will listen for connections on the given interface in the optional third argument.
If no such argument is given, it will listen on `127.0.0.1:6397` for the message server, and `127.0.0.1:6398` for the web server.

#### Client/Peer
`mgen-client [config.yaml]...`

`mgen-peer [hosts file] [config.yaml]...`

The client and peer configuration files are detailed below.

### Client Configuration

Clients are designed to simulate one user per configuration file, with multiple conversations.
The client can take multiple configuration files, and also accepts globs—similar to techniques used in TGen, a single client can simulate traffic of many individual users.
The following example configuration with explanatory comments should be enough to understand almost everything you need:

```YAML
# client-conversation.yaml

# A name used for logs and to create unique circuits for each user on a client.
user: "Alice"

# The <ip>:<port> of a socks5 proxy to connect through.
# Optional.
socks: "127.0.0.1:9050"

# The <address>:<port> of the message server,
# where <address> is an IP, onion address, or hostname.
# Can override in the conversation settings.
message_server: "server.maybe.onion:6397"

# Similarly, but for the web server (must include protocol).
web_server: "server.maybe.onion:6398"

# The number of seconds to wait until the client starts sending messages.
# This should be long enough that all clients have had time to start
# (sending messages to a client that isn't registered on the server is a
# fatal error), but short enough all conversations will have started by
# the experiment start.
# Can override in the conversation settings.
bootstrap: 5.0

# The number of seconds to wait after a network failure before retrying.
# Can override in the conversation settings.
retry: 5.0


# Parameters for distributions used by the Markov model.
# Can override in the conversation settings.
distributions:

  # Probability of Idle to Active transition with sent/received messages.
  s: 0.5
  r: 0.1

  # The distribution of message sizes, as measured in padding blocks.
  m: { distribution: "Poisson", lambda: 1.0 }

  # Distribution I, the amount of time Idle before sending a message.
  i: { distribution: "Normal", mean: 30.0, std_dev: 100.0 }

  # Distribution W, the amount of time Active without sending or receiving
  # messages to transition to Idle.
  w: { distribution: "Uniform", low: 0.0, high: 90.0 }

  # Distribution A_{s/r}, the time Active since last sent/received
  # message until the client sends a message.
  a_s: { distribution: "Exp", lambda: 2.0 }
  a_r: { distribution: "Pareto", scale: 1.0, shape: 3.0 }

# The list of conversations associated with the user.
conversations:

  # A conversation name used for logs, server-side lookups,
  # and unique circuits for each conversation,
  # even when two chats share the same participants.
  - group: "group1"
    # Most of the global settings can be overridden here.
```

Additional examples can be found in the [client shadow test configurations](/shadow/client/shadow.data.template/hosts).

The client currently supports six probability distributions for message timings: Normal and LogNormal, Uniform, Exp(onential), Pareto, and Weighted.
The parameter names can be found in the example above, except for Weighted (see the [Weighted section of this README](#weighted).
The distributions are sampled to return a double-precision floating point number of seconds.

The client currently supports five probability distributions for message sizes.
With their parameters, they are: Poisson (`lambda`: float), Binomial (`n`: integer, `p`: float), Geometric (`p`: float), Hypergeometric (`total_population_size`, `population_with_feature`, and `sample_size`: integers), and Weighted (see the [Weighted section of this README](#weighted)).
Floats and integer parameters are both 64-bits (i.e., double-precision floats and unsigned 64-bit ints, respectively).

The particular distributions and parameters used in the example are for demonstration purposes only; they have no relationship to empirical conversation behaviors.
When sampling, values below zero are clamped to 0—e.g., the `i` distribution above will have an outsize probability of yielding 0.0 seconds, instead of redistributing weight.
Any distribution in the [rand_distr](https://docs.rs/rand_distr/latest/rand_distr/index.html) crate would be simple to add support for.
Distributions not in that crate can also be supported, but would require implementing.

### Peer configuration

Running in peer-to-peer mode is very similar to running a client.
The main difference is that the first argument is the path of a "hosts file".
This file should have on each line an address:port and one or more user names, separated by whitespace (note that this is slightly different syntax to a real hosts file, as there is a port included).
When a peer connects to another peer, it uses the hosts file to look up the address to connect to.
See the minimal [example hosts file](/shadow/peer/shadow.data.template/hosts/hostsfile).
For consistency, this file should be shared by all hosts.

Aside from that, the only differences in the configuration are that recipients in a group must be listed, there is no server, and there is an optional `listen` configuration field to specify which interface the user should listen for connections on (if not provided, the interface given in the hosts file is used).
Here is an example peer conversation configuration (again, all values are for demonstration purposes only):

```YAML
# peer-conversation.yaml

user: "Alice"
socks: "127.0.0.1:9050"
listen: "127.0.0.1:6397"

conversations:
  - group: "group1"
    recipients: "Bob"
    bootstrap: 5.0
    retry: 5.0
    distributions:
      s: 0.5
      r: 0.1
      m: { distribution: "Poisson", lambda: 1.0 }
      i: { distribution: "Normal", mean: 30.0, std_dev: 100.0 }
      w: { distribution: "Normal", mean: 30.0, std_dev: 30.0 }
      a_s: { distribution: "Normal", mean: 10.0, std_dev: 5.0 }
      a_r: { distribution: "Normal", mean: 10.0, std_dev: 5.0 }
```

Additional examples can be found in the [peer shadow test configurations](/shadow/peer/shadow.data.template/hosts).

In the likely case that these peers are connecting via onion addresses, you must configure each torrc file to match with each peer configuration and hosts file
(in the above example, Alice's HiddenService lines in the torrc must have a `HiddenServicePort` line that forwards to `127.0.0.1:6397`, and the hosts file must have a line with `[string].onion:[port] Alice`, where "string" and "port" correspond to Alice's onion address and external port).
Multiple users can share an onion address by using different ports (different circuits will be used), though doing so will of course not simulate, e.g., additional load in Tor's distributed hash table.

### Weighted
Weighted distributions, which can be very large and commonly used across many clients or peers, are handled slightly differently from other distribution types.
`"Weighted"` can be used as the distribution type for both timing and message size distributions, with a single parameter of `weights_file`, which should be set to the path of the file storing the weights and values to be sampled from.
This file is a text file (technically, a CSV file), with two rows.
The first row is a series of non-negative integer weights, internally represented as 32-bit values that are then normalized into a probability distribution.
The second row is the corresponding values (32-bit integers for message sizes, double-precision floats for times).
When the distribution is sampled, the n'th value in the second row has a probability of being sampled equal to the n'th value of the first row divided by the sum of the first row.
The client/peer will abort if the two rows do not have the same number of elements.
Larger files will lead to slower initialization and higher memory overhead, but should still sample in constant time; see the docs for rand_distr's [WeightedAliasIndex](https://docs.rs/rand_distr/0.4.3/rand_distr/weighted_alias/struct.WeightedAliasIndex.html) for details.

## Testing

Unit tests can be run using the `cargo test` command.
Integration tests are performed using Shadow, so will require that it be installed, and probably in your environment's PATH.
You will also need the mgen executables to be in your environment's PATH for Shadow to find them (see the [installation instructions](#usage) above).
If you are running the Tor versions of the tests, you will also need a `tor` executable in your PATH.
Because the tests are invoked in Shadow, there is no simple way to run them with cargo, so they are instead run using standalone shell scripts.
You can find them in this project's [shadow directory](/shadow).
