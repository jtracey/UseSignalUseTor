This directory contains the code and data necessary to generate, configure, run, and plot Shadow experiments for simulated messenger traffic over Tor.
You will need working versions of Rust, python3, and bash.

The first step is to obtain the necessary data.
We provide some data in the `data/` directory, but not the ["Share and Multiply" (SaM) dataset](https://figshare.com/articles/dataset/WhatsApp_Data_Set/19785193).
Download the `json_files.zip` file they provide, and extract it somewhere.

You will also need to:
 - Install [Shadow](https://github.com/shadow/shadow) and [tornettools](https://github.com/shadow/tornettools).
 - Build and install MGen.
 - Build our version of tor.

Once that's done, perform the following steps, in order:
 - Run our `SaM-extractor`'s `extract` tool to pare and serialize the SaM data.
 - Use the tools in `hmm` to label messages as "active" or "idle".
 - Run `SaM-extractor`'s `process` tool to generate all empirical distributions other than message sizes.
 - Optionally: use our modified version of `whatsapp-public-groups` and `message_sizes.sh` script to gather file sizes from public group chats.
   - This requires a working WhatsApp account, with a valid phone number.
 - Run `SaM-extractor`'s `message-lens` tool to generate distributions for message sizes.
 - Run `generate-initial-networks.sh` to generate the initial Tor networks.
   - Open the file to see environment variables you can export to change experiment parameters.
 - Use the `patch-atlas.py` script in `mnettools` to patch a copy of the `atlas.gml` file; then replace the copy in the generated networks with it.
   - This will likely be integrated into the above script at some point.
 - Run the `generate-noproxy-networks.sh` scripts (with env vars set appropriately) to create modified versions of the initial Tor networks that also simulate unproxied mgen clients/servers.
 - Run the `generate-torproxy-networks.sh` scripts (with env vars set appropriately) to create modified versions of the noproxy networks that use Tor.
 - Run the `generate-noproxy-networks.sh` scripts (with env vars set appropriately) to create modified versions of the initial Tor networks that also simulate onion service mgen peers.
 - Run the experiments using Shadow. Note that, especially if run in parallel, this can take hundreds of GiB of RAM. A single 1% network (too small for statistical results, but large enough to test everything) takes about 30 GiB of RAM.
 - Use mgentools (in `MGen/mgentools/`) and tornettools to parse the data from the experiments.
 - Use mnettools `plot.py` and tornettools to plot the parsed data.
