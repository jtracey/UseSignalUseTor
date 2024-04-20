import numpy as np
import matplotlib.pyplot as plt

from scipy.stats import poisson
from hmmlearn import hmm

import sys

def get_states(counts, lens):
    if len(counts) == 0 or len(lens) == 0:
        return
    counts = np.array([c for c in counts])
    lens = np.array([l for l in lens])

    scores = list()
    models = list()
    for idx in range(10):  # ten different random starting states
        # define our hidden Markov model
        # (because we always prepend an hour of 0 messages,
        # and because it helps to ensure what the first state represents,
        # we set the probability of starting in the first state to 1,
        # and don't include start probability as a parameter to update)
        model = hmm.PoissonHMM(n_components=2, random_state=idx,
                               n_iter=10, params='tl', init_params='tl',
                               startprob_prior=np.array([1.0,0.0]),
                               lambdas_prior=np.array([[0.01], [0.1]]))
        model.startprob_ = np.array([1.0,0.0])
        model.fit(counts[:, None], lens)
        models.append(model)
        try:
            scores.append(model.score(counts[:, None], lens))
        except:
            print("igoring failed model scoring")

    # get the best model
    model = models[np.argmax(scores)]
    try:
        states = model.predict(counts[:, None], lens)
    except:
        print("failed to predict")
        return None, None
    if model.lambdas_[0] > model.lambdas_[1]:
        states = [int(not(s)) for s in states]

    return ','.join([str(s) for s in states]), ','.join([str(l) for l in model.lambdas_])

target_dir = sys.argv[1]
for i in range(2, len(sys.argv)):
    file_path = sys.argv[i]
    with open(file_path) as f:
        lines = f.readlines()

    counts = [int(n) for n in lines[0].strip().split(',')]
    lens = [int(n) for n in lines[1].strip().split(',')]

    states, lambdas = get_states(counts, lens)
    if states is None:
        continue

    file_out = target_dir + '/' + file_path.split('/')[-1]
    with open(file_out, 'w') as f:
        print(lines[0].strip(), file=f)
        print(lines[1].strip(), file=f)
        print(lines[2].strip(), file=f)
        print(lines[3].strip(), file=f)
        print(states, file=f)
        print(lambdas, file=f)
