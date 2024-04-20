import sys
import os
import argparse
import logging

from datetime import datetime
from random import randint
from random import seed as stdseed
from numpy.random import seed as numpyseed
from multiprocessing import cpu_count
from platform import platform, uname

from tornettools.util import which, make_directories
from tornettools._version import __version__

import re
import os
import logging

from itertools import cycle
import matplotlib.pyplot as pyplot
from matplotlib.ticker import FuncFormatter
from matplotlib.backends.backend_pdf import PdfPages

from tornettools.util import load_json_data, find_matching_files_in_dir

from tornettools.plot_common import (DEFAULT_COLORS, DEFAULT_LINESTYLES, draw_cdf, draw_cdf_ci,
                                     draw_line, draw_line_ci, quantile, set_plot_options)
from tornettools.plot_tgen import plot_tgen
from tornettools.plot_oniontrace import plot_oniontrace



HELP_MAIN = """
Use 'tornettools <subcommand> --help' for more info
"""
DESC_MAIN = """
tornettools is a utility to guide you through the Tor network
experimentation process using Shadow. tornettools must be run with a
subcommand to specify a mode of operation.

For more information, see https://github.com/shadow/tornettools.
"""

HELP_STAGE = """
Process Tor metrics data for staging network generation
"""
DESC_STAGE = """
Process Tor network consensuses, relay descriptors, and user files
from Tor metrics to stage TorNet network generation.

This command should be used before running generate. This command
produces staging files that will be required for the generate
command to succeed.
"""

HELP_GENERATE = """
Generate TorNet network configurations
"""
DESC_GENERATE = """
Loads the TorNet staging files produced with the stage command
and uses them to generate a valid TorNet network configuration.

This command should be used after running stage.
"""

HELP_SIMULATE = """
Run a TorNet simulation in Shadow
"""
DESC_SIMULATE = """
Runs a Tor simulation using Shadow and the TorNet network
configurations files generated with the generate command.

This command should be used after running generate.
"""

HELP_PARSE = """
Parse useful data from simulation log files
"""
DESC_PARSE = """
Parses log files created by simulations run with the simulate
command; extracts and stores various useful performance metrics.

This command should be used after running simulate.
"""

HELP_PLOT = """
Plot previously parsed data to visualize results
"""
DESC_PLOT = """
Visualizes various performance metrics that were extracted and
stored with the parse command by producing graphical plots.

This command should be used after running parse.
"""

HELP_ARCHIVE = """
Cleanup and compress Shadow simulation data
"""
DESC_ARCHIVE = """
Prepares a Shadow simulation directory for archival by compressing
simulation output log files and data directories.

This command can be used any time after running simulate, but
ideally after parsing and plotting is also completed.
"""

def __setup_logging_helper(logfilename=None):
    my_handlers = []

    stdout_handler = logging.StreamHandler(sys.stdout)
    my_handlers.append(stdout_handler)

    if logfilename != None:
        make_directories(logfilename)
        file_handler = logging.FileHandler(filename=logfilename)
        my_handlers.append(file_handler)

    logging.basicConfig(
        level=logging.INFO,
        format='%(asctime)s %(created)f [tornettools] [%(levelname)s] %(message)s',
        datefmt='%Y-%m-%d %H:%M:%S',
        handlers=my_handlers,
    )

    msg = "Logging system initialized! Logging events to stdout"
    if logfilename != None:
        msg += " and to '{}'".format(logfilename)
    logging.info(msg)

def __setup_logging(args):
    if args.quiet <= 1:
        logfilename = None
        if args.quiet == 0 and hasattr(args, 'prefix'):
            # log to a file too
            prefixstr = str(args.prefix)
            funcstr = str(args.command) if args.command is not None else "none"
            datestr = datetime.now().strftime("%Y-%m-%d.%H.%M.%S")
            logfilename = "{}/tornettools.{}.{}.log".format(prefixstr, funcstr, datestr)
        __setup_logging_helper(logfilename)
    else:
        pass # no logging

def run(args):
    logging.info("Plotting simulation results now")
    set_plot_options()

    logging.info("Plotting mgen comparisons")
    __plot_mnet(args)

    logging.info(f"Done plotting! PDF files are saved to {args.prefix}")

def __pattern_for_basename(circuittype, basename):
    s = basename + r'\.' + circuittype + r'\.json'
    if circuittype == 'exit':
        # Data files without a circuittype contain exit circuits (from legacy
        # tornettools runs).
        s = basename + r'(\.' + circuittype + r')?\.json'
    else:
        s = basename + r'\.' + circuittype + r'\.json'
    return re.compile(s)

def __plot_mnet(args):
    args.pdfpages = PdfPages(f"{args.prefix}/tornet.plot.pages.pdf")

    net_scale = __get_simulated_network_scale(args)

    logging.info("Loading mgen rtt_all data")
    dbs = __load_tornet_datasets(args, "rtt_all.mgen.json")
    logging.info("Plotting mgen rtt_all")
    __plot_mgen_rtt_all(args, dbs, net_scale)

    logging.info("Loading mgen rtt_timeout data")
    dbs = __load_tornet_datasets(args, "rtt_timeout.mgen.json")
    logging.info("Plotting mgen rtt_timeout")
    __plot_mgen_rtt_timeout(args, dbs, net_scale)

    logging.info("Loading mgen timeout_by_send data")
    dbs = __load_tornet_datasets(args, "timeout_by_send.mgen.json")
    logging.info("Plotting mgen rtt_by_send")
    __plot_mgen_timeout_by_send(args, dbs, net_scale)

    logging.info("Loading mgen timeout_by_receive data")
    dbs = __load_tornet_datasets(args, "timeout_by_receive.mgen.json")
    logging.info("Plotting mgen rtt_by_receive")
    __plot_mgen_timeout_by_receive(args, dbs, net_scale)

    logging.info("Loading mgen rtt_counts data")
    dbs = __load_tornet_datasets(args, "counts.mgen.json")
    logging.info("Plotting mgen rtt_counts")
    __plot_mgen_count(args, dbs, net_scale)

    args.pdfpages.close()


def __plot_mgen_rtt_all(args, rtt_dbs, net_scale):
    # cache the corresponding data in the 'data' keyword for __plot_cdf_figure
    for rtt_db in rtt_dbs:
        rtt_db['data'] = rtt_db['dataset']
    __plot_cdf_figure(args, rtt_dbs, 'rtt_all.mgen', yscale='taillog',
                      xscale='log',
                      xlabel="Time (s)")

def __plot_mgen_rtt_timeout(args, rtt_dbs, net_scale):
    # cache the corresponding data in the 'data' keyword for __plot_cdf_figure
    for rtt_db in rtt_dbs:
        rtt_db['data'] = rtt_db['dataset']
    __plot_cdf_figure(args, rtt_dbs, 'rtt_timeout.mgen', yscale='taillog',
                      xlabel="Time (s)")


def __plot_mgen_timeout_by_send(args, rtt_dbs, net_scale):
    # cache the corresponding data in the 'data' keyword for __plot_cdf_figure
    for rtt_db in rtt_dbs:
        rtt_db['data'] = rtt_db['dataset']
    __plot_cdf_figure(args, rtt_dbs, 'timeout_by_send.mgen', yscale='taillog',
                      xscale='log',
                      xlabel="Fraction of (user, group)'s expected receipts")

def __plot_mgen_timeout_by_receive(args, rtt_dbs, net_scale):
    # cache the corresponding data in the 'data' keyword for __plot_cdf_figure
    for rtt_db in rtt_dbs:
        rtt_db['data'] = rtt_db['dataset']
    __plot_cdf_figure(args, rtt_dbs, 'timeout_by_receive.mgen', yscale='taillog',
                      xscale='log',
                      xlabel="Fraction of (user, group)'s receipts")


def __plot_mgen_count(args, count_dbs, net_scale):
    # cache the corresponding data in the 'data' keyword for __plot_cdf_figure
    for count_db in count_dbs:
        count_db['data'] = count_db['dataset']
    __plot_cdf_figure(args, count_dbs, 'count.mgen',
                      xlabel="Messages sent per user")

def __plot_cdf_figure(args, dbs, filename, xscale=None, yscale=None, xlabel=None, ylabel="CDF"):
    color_cycle = cycle(DEFAULT_COLORS)
    linestyle_cycle = cycle(DEFAULT_LINESTYLES)

    pyplot.figure()
    lines, labels = [], []

    for db in dbs:
        if 'data' not in db or len(db['data']) < 1:
            continue
        elif len(db['data']) == 1:
            (plot_func, d) = draw_cdf, db['data'][0]
        else:
            (plot_func, d) = draw_cdf_ci, db['data']

        if len(d) < 1:
            continue

        line = plot_func(pyplot, d,
                         yscale=yscale,
                         label=db['label'],
                         color=db['color'] or next(color_cycle),
                         linestyle=next(linestyle_cycle))

        lines.append(line)
        labels.append(db['label'])

    if xscale is not None:
        pyplot.xscale(xscale)
        if xlabel is not None:
            xlabel += __get_scale_suffix(xscale)
    if yscale is not None:
        pyplot.yscale(yscale)
        if ylabel is not None:
            ylabel += __get_scale_suffix(yscale)
    if xlabel is not None:
        pyplot.xlabel(xlabel, fontsize=14)
    if ylabel is not None:
        pyplot.ylabel(ylabel, fontsize=14)

    m = 0.025
    pyplot.margins(m)

    # the plot will exit the visible space at the 99th percentile,
    # so make sure the x-axis is centered correctly
    # (this is usually only a problem if using the 'taillog' yscale)
    x_visible_max = None
    for db in dbs:
        if len(db['data']) >= 1 and len(db['data'][0]) >= 1:
            q = quantile(db['data'][0], 0.99)
            x_visible_max = q if x_visible_max is None else max(x_visible_max, q)
    if x_visible_max is not None:
        pyplot.xlim(xmin=max(0, -m * x_visible_max), xmax=(m + 1) * x_visible_max)

    __plot_finish(args, lines, labels, filename)

def __plot_finish(args, lines, labels, filename):
    pyplot.tick_params(axis='y', which='major', labelsize=12)
    pyplot.tick_params(axis='x', which='major', labelsize=14)
    pyplot.tick_params(axis='both', which='minor', labelsize=8)
    pyplot.grid(True, axis='both', which='minor', color='0.1', linestyle=':', linewidth='0.5')
    pyplot.grid(True, axis='both', which='major', color='0.1', linestyle=':', linewidth='1.0')

    pyplot.legend(lines, labels, loc='lower right', fontsize=14)
    pyplot.tight_layout(pad=0.3)
    pyplot.savefig(f"{args.prefix}/{filename}.{'png' if args.plot_pngs else 'pdf'}")
    args.pdfpages.savefig()

def __get_scale_suffix(scale):
    if scale == 'taillog':
        return " (tail log scale)"
    elif scale == 'log':
        return " (log scale)"
    else:
        return ""

def __time_format_func(x, pos):
    hours = int(x // 3600)
    minutes = int((x % 3600) // 60)
    seconds = int(x % 60)
    return "{:d}:{:02d}:{:02d}".format(hours, minutes, seconds)

def __load_tornet_datasets(args, filepattern):
    tornet_dbs = []

    print(args.labels)
    label_cycle = cycle(args.labels) if args.labels is not None else None
    color_cycle = cycle(args.colors) if args.colors is not None else None

    if args.tornet_collection_path is not None:
        for collection_dir in args.tornet_collection_path:
            tornet_db = {
                'dataset': [load_json_data(p) for p in find_matching_files_in_dir(collection_dir, filepattern)],
                'label': next(label_cycle) if label_cycle is not None else os.path.basename(collection_dir),
                'color': next(color_cycle) if color_cycle is not None else None,
            }
            tornet_dbs.append(tornet_db)

    return tornet_dbs

def __load_torperf_datasets(torperf_argset):
    torperf_dbs = []

    if torperf_argset is not None:
        for torperf_args in torperf_argset:
            torperf_db = {
                'dataset': load_json_data(torperf_args[0]) if torperf_args[0] is not None else None,
                'label': torperf_args[1] if torperf_args[1] is not None else "Public Tor",
                'color': torperf_args[2],
            }
            torperf_dbs.append(torperf_db)

    return torperf_dbs

def __get_simulated_network_scale(args):
    sim_info = __load_tornet_datasets(args, "simulation_info.json")

    net_scale = 0.0
    for db in sim_info:
        for i, d in enumerate(db['dataset']):
            if 'net_scale' in d:
                if net_scale == 0.0:
                    net_scale = float(d['net_scale'])
                    logging.info(f"Found simulated network scale {net_scale}")
                else:
                    if float(d['net_scale']) != net_scale:
                        logging.warning("Some of your tornet data is from networks of different scale")
                        logging.critical(f"Found network scales {net_scale} and {float(d['net_scale'])} and they don't match")

    return net_scale

def __compute_torperf_error_rates(daily_counts):
    err_rates = []
    for day in daily_counts:
        total = int(daily_counts[day]['requests'])
        if total <= 0:
            continue

        timeouts = int(daily_counts[day]['timeouts'])
        failures = int(daily_counts[day]['failures'])

        err_rates.append((timeouts + failures) / float(total) * 100.0)
    return err_rates


def main():
    my_formatter_class = CustomHelpFormatter

    # construct the options
    main_parser = argparse.ArgumentParser(description=DESC_MAIN, formatter_class=my_formatter_class)

    main_parser.add_argument('-v', '--version',
        help="""Prints the version of the toolkit and exits.""",
        action="store_true", dest="do_version",
        default=False)

    main_parser.add_argument('-q', '--quiet',
        help="""Do not write log messages to file. Use twice to also not write to stdout.""",
        action="count", dest="quiet",
        default=0)

    main_parser.add_argument('-s', '--seed',
        help="""Initialize tornettools' PRNGs with a seed to allow for
            deterministic behavior. This does not affect the seed for the Shadow
            simulation.""",
        action="store", type=int, dest="seed", metavar="N",
        default=None)

    sub_parser = main_parser.add_subparsers(help=HELP_MAIN, dest='command')

    plot_parser = sub_parser.add_parser('plot',
        description=DESC_PLOT,
        help=HELP_PLOT,
        formatter_class=my_formatter_class)
    plot_parser.set_defaults(func=run, formatter_class=my_formatter_class)

    plot_parser.add_argument('tornet_collection_path',
        help="""Path to a directory containing one or more subdirectories of parsed
            tornet results from the 'parse' command. Confidence intervals are drawn
            when this path contains plot data from multiple simulations.""",
        action='store',
        type=__type_str_dir_path_in,
        nargs='+')

    plot_parser.add_argument('-t', '--tor_metrics_path',
        help="""Path to a tor_metrics.json file that was created by the 'stage' command,
            which we be compared against the tornet collections. The label and color
            to use in the graphs that we create are optional.""",
        action=PathStringArgsAction,
        nargs='+',
        metavar="PATH [LABEL [COLOR]]")

    plot_parser.add_argument('--prefix',
        help="""A directory PATH prefix where the graphs generated by this script
            will be written.""",
        action="store",
        type=__type_str_dir_path_out,
        dest="prefix",
        default=os.getcwd(),
        metavar="PATH")

    plot_parser.add_argument('-l', '--labels',
        help="""Labels for the tornet collections to be used in the graph legends.""",
        action='store',
        type=str,
        dest="labels",
        nargs='+',
        metavar='LABEL')

    plot_parser.add_argument('-c', '--colors',
        help="""Colors for the tornet collections to be used in the graph plots.""",
        action='store',
        type=str,
        dest="colors",
        nargs='+',
        metavar='COLOR')

    plot_parser.add_argument('-a', '--all',
        help="""Also generate individual tgentools and oniontracetools plots for each simulation.""",
        action="store_true",
        dest="plot_all",
        default=False)

    plot_parser.add_argument('--pngs',
        help="""Save individual plot images in png instead of pdf format.""",
        action="store_true",
        dest="plot_pngs",
        default=False)

    # get args and call the command handler for the chosen mode
    args = main_parser.parse_args()

    if not hasattr(args, "prefix") and hasattr(args, "tornet_config_path"):
        args.prefix = args.tornet_config_path
    if hasattr(args, "nprocesses"):
        args.nprocesses = args.nprocesses if args.nprocesses > 0 else cpu_count()

    # check if it's just a version check and we should short circuit
    if args.do_version:
        __setup_logging(args)
        logging.info("tornettools version {}".format(__version__))
        return

    # if it's anything other than version, we need a subcommand
    if args.command == None:
        main_parser.print_usage()
        return

    # now we know we can start
    __setup_logging(args)

    # seed the pseudo-random generators
    # if we don't have a seed, choose one and make sure we log it for reproducibility
    if args.seed == None:
        args.seed = randint(0, 2**31)
    stdseed(args.seed)
    numpyseed(args.seed)
    logging.info("Seeded standard and numpy PRNGs with seed={}".format(args.seed))

    logging.info("The argument namespace is: {}".format(str(args)))
    logging.info("The platform is: {}".format(str(platform())))
    logging.info("System info: {}".format(str(uname())))

    # now run the configured mode
    rv = run(args)

    if rv == 0 or rv == None:
        return 0
    elif isinstance(rv, int):
        return rv
    else:
        logging.warning(f"Unknown return value: {rv}")
        return 1


def __type_nonnegative_integer(value):
    i = int(value)
    if i < 0:
        raise argparse.ArgumentTypeError("'%s' is an invalid non-negative int value" % value)
    return i

def __type_nonnegative_float(value):
    i = float(value)
    if i < 0.0:
        raise argparse.ArgumentTypeError("'%s' is an invalid non-negative flat value" % value)
    return i

def __type_fractional_float(value):
    i = float(value)
    if i <= 0.0 or i > 1.0:
        raise argparse.ArgumentTypeError("'%s' is an invalid fractional float value" % value)
    return i

def __type_str_file_path_out(value):
    s = str(value)
    if s == "-":
        return s
    p = os.path.abspath(os.path.expanduser(s))
    make_directories(p)
    return p

def __type_str_dir_path_out(value):
    s = str(value)
    p = os.path.abspath(os.path.expanduser(s))
    make_directories(p)
    return p

def __type_str_file_path_in(value):
    s = str(value)
    if s == "-":
        return s
    p = os.path.abspath(os.path.expanduser(s))
    if not os.path.exists(p):
        raise argparse.ArgumentTypeError(f"Path does not exist: {p}")
    elif not os.path.isfile(p):
        raise argparse.ArgumentTypeError(f"Path is not a file: {p}")
    return p

def __type_str_dir_path_in(value):
    s = str(value)
    p = os.path.abspath(os.path.expanduser(s))
    if not os.path.exists(p):
        raise argparse.ArgumentTypeError(f"Path does not exist: {p}")
    elif not os.path.isdir(p):
        raise argparse.ArgumentTypeError(f"Path is not a directory: {p}")
    return p

def type_str_file_path_in(p):
    return __type_str_file_path_in(p)

# adds the 'RawDescriptionHelpFormatter' to the ArgsDefault one
class CustomHelpFormatter(argparse.ArgumentDefaultsHelpFormatter):
    def _fill_text(self, text, width, indent):
        return ''.join([indent + line for line in text.splitlines(True)])

# a custom action for passing in experimental data directories when plotting
class PathStringArgsAction(argparse.Action):
    def __call__(self, parser, namespace, values, option_string=None):
        if len(values) == 0:
            raise argparse.ArgumentError(self, "A path is required.")
        elif len(values) > 3:
            raise argparse.ArgumentError(self, "Must specify 3 or fewer strings.")

        # get the values
        path = values[0]
        label = values[1] if len(values) > 1 else None
        color = values[2] if len(values) > 2 else None

        # extract and validate the path
        path = type_str_file_path_in(path)

        # remove the default
        if "_didremovedefault" not in namespace:
            setattr(namespace, self.dest, [])
            setattr(namespace, "_didremovedefault", True)

        # append our new arg set
        dest = getattr(namespace, self.dest)
        dest.append([path, label, color])

if __name__ == '__main__':
    sys.exit(main())
