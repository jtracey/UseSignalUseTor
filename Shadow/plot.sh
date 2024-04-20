source ~/Code/archive/tornettools/toolsenv/bin/activate
tornettools plot "no proxy-10000" "no proxy-100000" "ind. circuits-10000" "shared circuits-10000" "shared circuits-100000" "onion services-10000"
python3 ~/Code/archive/tornettools/tornettools/plot_mgen.py plot "no proxy-10000" "no proxy-100000" "ind. circuits-10000" "shared circuits-10000" "shared circuits-100000" "onion services-10000"
