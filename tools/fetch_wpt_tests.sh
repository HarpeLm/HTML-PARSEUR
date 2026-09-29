#!/bin/sh
# Récupère UNIQUEMENT les tests de construction d'arbre de WPT (web-platform-tests)
# dans wpt-tests/ (ignoré par git). Le reste de WPT fait plusieurs Go.
#
#   ./tools/fetch_wpt_tests.sh
set -e
DIR=wpt-tests
if [ ! -d "$DIR/.git" ]; then
  git clone --filter=blob:none --no-checkout --depth 1 \
    https://github.com/web-platform-tests/wpt.git "$DIR"
  git -C "$DIR" sparse-checkout set --no-cone /html/syntax/parsing/resources/
  git -C "$DIR" checkout
fi
git -C "$DIR" log -1 --format="WPT %h (%cs)"
ls "$DIR/html/syntax/parsing/resources/"*.dat | wc -l | xargs echo "fichiers .dat :"
