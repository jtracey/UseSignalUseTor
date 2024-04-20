Usage:

`python3 get_w.py target_dir stats_file1 [stats_file2 ...]`

where each `stats_file` is the output of SaM extractor's extract for a user; or, to run on all CPU cores:

`./parallel_run.sh stats_dir_in stats_dir_out`

Once a user has had their messages labeled here, SaM extractor's process command can be run on the output.


## Design

Ideally, users in our simulation would generate conversations according to some sort of large machine learning model that takes as input all previous inter-arrival times (IATs, the time from this message since a previous message was last sent or received) and message sizes, and predicts this user's next message size and response time.
Such a model would easily be impossible to scale to large sizes.
Instead, we simplify to four states: idle, idle and just sent a message, idle and just received a message, and active.
See the state machine in the paper, which can be thought of as a slightly modified Markov model with two notions of time (namely, real time, which is continuous, and receiving messages, which is discrete).
Alternatively, it can be thought of as a true Markov model with states that grow linearly with the number of participants in the conversation (where receiving a message is instead represented by "another user's state" sending a message), but this would create far too many state transitions to model in practice.

Initially, it would appear a Hidden Markov Model (HMM) or extension would be well-suited to finding all the parameters of our state machine.
Unfortunately, there is no established way to train HMMs or their extensions with multiple notions of time (namely, continuous and discrete forms of time).
Specifically, there is no efficient way to construct a HMM where state transitions may occur because *either* some time has passed or a message was received.

Rather than attempt to find abstract models for distributions then, we instead opt to use empirical distributions (i.e., sampling directly from recorded values) wherever possible.
In order to do this, however, we must first determine a way to categorize messages so we know when to sample from which.
To avoid the problem of multiple notions of time, we temporarily discard any notion of receiving messages, and reduce our state machine to two states: idle and active.
We then index all messages a user sent into conversations: in each minute of each conversation, count the number of messages the user sent.
Because our simulations are not intended to simulate more than an hour of traffic, and because of how sparse conversations can be (e.g., using data indexed by the minute over the course of multiple years can become expensive), conversations are further broken down into fragments, where all fragments start and end with an hour of 0 message counts (or the end of the conversation, whichever came first), and any place in the conversation where more than two hours since the last message was sent gets broken into two fragments.
These fragments are then fed into a HMM learning algorithm (with Poisson distribution emissions) for each user, which generates a transition matrix.
Once we have that, we can predict the state for each message, and label it accordingly, for use in generating the empirical distributions we will actually simulate with.
