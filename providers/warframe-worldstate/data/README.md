# Label data provenance

## WFCD snapshots

`sol_nodes.json` and `mission_types.json` are whitespace/key-order normalized
snapshots of these canonical files:

- <https://github.com/WFCD/warframe-worldstate-data/blob/84516821559388df0383bcacea04ca2b9e93b20d/data/solNodes.json>
- <https://github.com/WFCD/warframe-worldstate-data/blob/84516821559388df0383bcacea04ca2b9e93b20d/data/missionTypes.json>

Snapshot revision: `84516821559388df0383bcacea04ca2b9e93b20d`.
After canonical JSON normalization with `jq -S -c`, the upstream/local SHA-256
digests are respectively:

- sol nodes: `f9430f52829fa821828a9c8201d713dd2cc185621048c5df2037f527e75ee295`;
- mission types: `ce2c35c0c8337cad32ba1c872f9094f25654069c0d50fa20e8a7dd3bf0a83464`.

The canonical repository is maintained by Warframe Community Developers
(WFCD) and is MIT licensed, copyright (c) 2016 Matej Voboril. The required
notice is retained verbatim in [`WFCD-LICENSE`](WFCD-LICENSE). MIT permits use,
modification, and distribution when that notice is retained, so these data are
compatible with distribution alongside the `AGPL-3.0-only` provider. This does
not transfer or relicense the upstream copyright.

## PlayerVox projections

`factions.json`, `item_names.json`, `sortie_bosses.json`, and
`sortie_modifiers.json` are small PlayerVox-curated display projections, based
in part on the same WFCD factual tables and public Warframe identifiers. They
are not represented as verbatim WFCD snapshots. Their exact source version was
first committed by Valhallab SASU in PlayerVox OverCrow commit
`23f9dfb10c5b35a1d6d544e88fa874615f9fae5e` under `AGPL-3.0-only`; this port
copies those four files unchanged and retains the WFCD notice for the inputs.

The provider performs no runtime label download. Updating any snapshot requires
source and license review, updated revision/digests here, bounded-data tests,
and a rebuilt component hash. Unknown codes use a bounded readable fallback.
