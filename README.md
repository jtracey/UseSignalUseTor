# Artifact for "Use Signal, Use Tor?"

This repository serves as a single home for all code written or modified for use in the "Use Signal, Use Tor?: Making Messengers Mask Metadata" paper.
It is intended as a place to host the code as it was used for this research, and not further active development.
The respective components of this research will be or have been broken out into their own repositories, for easier maintenance, development, and use by other researchers.

## Structure

The repo is divided into two directories for two sections of the paper:
 - [**Traffic-Analysis**](Traffic-Analysis/README.md), which corresponds to the work in Section 3 of the paper, is everything needed to MITM Signal client-server connections on Android emulators.
 - [**Shadow**](Shadow/README.md), which corresponds to the work in Section 4 of the paper, is everything needed to configure, generate, run, parse, and plot the Shadow experiments presented in that section.

This separation is natural, as even though the analysis done in the former directly informed the design and some parameters of the code in latter, the data from the former is not directly fed as inputs to the latter.
The one place where code from both parts may help is collecting file sizes from public WhatsApp groups—this is used in the Shadow portion, but if you do not have a rooted Android device you are willing to have a sacrificial WhatsApp install on, the tools and instructions for setting up a rooted Android emulator in the Traffic-Analysis part will be useful.


## License

Most of this repository is licensed under the terms of the MIT License, found in the [LICENSE](LICENSE) file.
The major exceptions are:
 - The [protobufs](Traffic-Analysis/protobufs/), which are unmodified from Signal's copies, and are available under the terms of the AGPL.
 - The [MagiskOnEmulator](Traffic-Analysis/MagiskOnEmulator) code, which is Apache licensed and modified from [the original](https://github.com/shakalaca/MagiskOnEmulator).
 
There should be a copy of [whatsapp-public-groups](https://github.com/gvrkiran/whatsapp-public-groups), from the WhatsApp Doc paper, in the [`Shadow`](Shadow/) directory, but no license is available for that repo.
