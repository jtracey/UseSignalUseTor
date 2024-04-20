# Artifact for "Use Signal, Use Tor?"

This repository serves as a single home for all code written or modified for use in the "Use Signal, Use Tor?: Making Messengers Mask Metadata" paper.
It is intended as a place to host the code as it was used for this research, and not further active development.
The respective components of this research will be or have been broken out into their own repositories, for easier maintenance, development, and use by other researchers.

## Structure

The repo is divided into two directories for two sections of the paper:
 - **Traffic-Analysis**, which corresponds to the work in Section 3 of the paper, is everything needed to MITM Signal client-server connections on Android emulators.
 - **Shadow**, which corresponds to the work in Section 4 of the paper, is everything needed to configure, generate, run, parse, and plot the Shadow experiments presented in that section.

This separation is natural, as even though the analysis done in the former directly informed the design and some parameters of the code in latter, the data from the former is not directly fed as inputs to the latter.
The one place where code from both parts may help is collecting file sizes from public WhatsApp groups—this is used in the Shadow portion, but if you do not have a rooted Android device you are willing to have a sacrificial WhatsApp install on, the tools and instructions for setting up a rooted Android emulator in the Traffic-Analysis part will be useful.


## License

As stated before, we do not recommend using the code as it exists in this repo for additional research.
We will update it with links to the individual repos as they are made public.
This repo currently includes the entirety of our modified copy of the WhatsApp Doc code (`Shadow/whatsapp-public-groups/`), which does not currently have a license.
The repo in its entirety, and that code in particular, should therefore not be distributed outside its capacity as a research artifact, at least until this is resolved with its original authors.
Other directories may contain code with their own respective licenses, and can be redistributed accordingly.
