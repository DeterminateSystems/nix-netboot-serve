#!/bin/sh

set -eu

storepath=$1
cachedir=./cpio-cache/
cachekey=$cachedir/$(basename "$storepath").cpio.gz
lockkey=$cachekey.lock

log() {
  echo "$@" >&2
}

if [ -e "$cachekey" ]; then
  log "Cache hit, first try on $storepath"
  echo -n "$cachekey"
  exit 0
fi

try=0
while true; do
  try=$((try + 1))
  if [ -e "$cachekey" ]; then
    log "($try) Found cached"
    echo -n "$cachekey"
    exit 0
  fi

  if mkdir "$lockkey"; then
    log "($try) Acquired lock to generate $storepath"

    if [ -e "$cachekey" ]; then
      log "($try) Acquired lock to discover it was generated before us for $storepath"
      echo -n "$cachekey"
      exit 0
    fi

    (
      # go to / and find .$storepath so it is ./nix/store/... which will
      # create relative paths that the kernel craves in its cpios
      # note: this requires /nix then /nix/store already by another
      # cpio archive.
      cd "/"
      find ".$storepath" -print0 \
          | sort -z \
          | cpio -o -H newc -R +0:+1 --reproducible --null \
          | gzip -9n
    ) > "$cachekey"
    log "($try) Generated $storepath"
    echo -n "$cachekey"
    rmdir "$lockkey"
    exit 0
  else
    log "($try) Failed to lock on $storepath, sleep 1s"
    sleep 1
  fi
done
