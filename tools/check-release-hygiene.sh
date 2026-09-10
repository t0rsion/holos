#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

failed=0

path_hits="$({
    git grep -n -I -E '/home/[^/[:space:]]+|/Users/[^/[:space:]]+|[A-Za-z]:\\Users\\[^\\[:space:]]+' \
        -- . ':(exclude)tools/check-release-hygiene.sh' || true
} | grep -vF '/home/linuxbrew/.linuxbrew' || true)"
if [[ -n "$path_hits" ]]; then
    echo "release hygiene: a tracked file contains a local user path" >&2
    printf '%s\n' "$path_hits" >&2
    failed=1
fi

email_hits="$({
    git grep -n -I -E '[[:alnum:]._%+-]+@[[:alnum:].-]+\.[A-Za-z]{2,}' -- . || true
} | grep -vF 'admin+bot@axo.dev' || true)"
if [[ -n "$email_hits" ]]; then
    echo "release hygiene: a tracked file contains an unapproved email" >&2
    printf '%s\n' "$email_hits" >&2
    failed=1
fi

secret_hits="$(git grep -n -I -E \
    'BEGIN (RSA |OPENSSH |EC )?PRIVATE KEY|gh[pousr]_[A-Za-z0-9_]{20,}|github_pat_[A-Za-z0-9_]{20,}|AKIA[0-9A-Z]{16}|Authorization:[[:space:]]*(Bearer|Basic)[[:space:]]+[A-Za-z0-9._~+/-]{12,}' \
    -- . || true)"
if [[ -n "$secret_hits" ]]; then
    echo "release hygiene: a tracked file resembles a secret" >&2
    printf '%s\n' "$secret_hits" >&2
    failed=1
fi

# Check the prose surfaces for high-confidence release identity references.
# Manifests, workflows, benchmark provenance, generated records, and the
# changelog carry technical identities by design, so this report does not fail
# the release on its own.
identity_hits="$(
    git grep -n -I -E \
        '(^|[^[:alnum:]])(commit|branch)[[:space:]]+([0-9a-f]{7,40}|main|master|develop|v[0-9][^[:space:]]*)|(^|[^[:alnum:]])version[[:space:]]+v?[0-9]+\.[0-9]+\.[0-9]+([^[:alnum:]]|$)' \
        -- README.md crates/holos-tda-py/README.md '*.rs' '*.py' '*.sh' \
        ':(exclude)benchmarks/**' ':(exclude).github/**' \
        ':(exclude)CHANGELOG.md' ':(exclude)Cargo.lock' \
        ':(exclude)*Cargo.toml' ':(exclude)*pyproject.toml' || true
)"
if [[ -n "$identity_hits" ]]; then
    echo "release hygiene: review these public identity references (report only)" >&2
    printf '%s\n' "$identity_hits" >&2
fi

for document in README.md CHANGELOG.md benchmarks/README.md \
    crates/holos-tda-py/README.md
do
    if ! awk '
        /^```/ { fence = !fence; next }
        fence || /^\|/ || /^\[!\[/ || /^\[[^]]+\]: https?:/ { next }
        length($0) > 79 {
            printf "%s:%d: prose line has %d columns\n", FILENAME, FNR, length($0)
            bad = 1
        }
        END { exit bad }
    ' "$document"
    then
        failed=1
    fi
done

if ((failed)); then
    exit 1
fi

echo "release hygiene: checked tracked paths, emails, secrets, identity references, and prose wrapping"
