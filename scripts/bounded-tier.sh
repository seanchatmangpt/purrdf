#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
# Bounded stage runner — CI mirror of the local 5-second bounded-build law.
# Every stage runs under `timeout -k 1 <secs>` and prints
# `RECEIPT tier=N stage=<name> status=PASS|TIMEOUT|FAIL ms=<ms> sla=<ms>`.
# TIMEOUT (rc 124/137) and FAIL both exit nonzero; there is no
# continue-on-error and no unbounded stage.

set -u

tier="$1" stage="$2" secs="$3" sla="$4"; shift 4
rc=0
started="$(date +%s%N)"
timeout -k 1 "${secs}" "$@" || rc=$?
ended="$(date +%s%N)"
ms=$(( (ended - started) / 1000000 ))
if [ "${rc}" -eq 0 ]; then
  status=PASS
elif [ "${rc}" -eq 124 ] || [ "${rc}" -eq 137 ]; then
  status=TIMEOUT
else
  status=FAIL
fi
echo "RECEIPT tier=${tier} stage=${stage} status=${status} ms=${ms} sla=${sla}"
[ "${rc}" -eq 0 ] || exit 1
