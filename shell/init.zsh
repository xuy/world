# world shell init — aliases + dynamic completions
#
# Add to ~/.zshrc:
#   source /path/to/world/shell/init.zsh
#
# Or, if world is on your PATH and you cloned to a known location:
#   source ~/src/world/shell/init.zsh

# ─── Aliases ──────────────────────────────────────────────────────────────────
# Short forms: wo = world observe, wa = world act, etc.

alias wo='world observe'
alias wa='world act'
alias ws='world spec'
alias ww='world await'
alias wsample='world sample'

# ─── Completion ───────────────────────────────────────────────────────────────

# Resolve the world project root at source time.
# Works whether sourced from .zshrc or interactively.
_world_root="${${(%):-%x}:A:h:h}"
if [[ ! -d "$_world_root/plugins" ]]; then
  # Fallback: try relative to the world binary
  _world_root="$(dirname "$(whence -p world 2>/dev/null)")/.."
  # Also check two levels up (for target/release/world)
  if [[ ! -d "$_world_root/plugins" ]]; then
    _world_root="$(dirname "$(whence -p world 2>/dev/null)")/../../.."
  fi
fi

_world_plugins_dir() {
  local dir="$_world_root/plugins"
  [[ -d "$dir" ]] && echo "$dir"
}

# Cache domain list (native + plugins)
_world_domains() {
  local -a domains
  # Native domains (compiled in)
  domains=(process network container service disk brew printer log)
  # Plugin domains (discovered from filesystem)
  local pdir="$(_world_plugins_dir)"
  if [[ -n "$pdir" && -d "$pdir" ]]; then
    for d in "$pdir"/*/spec.json(N); do
      domains+=(${d:h:t})
    done
  fi
  # Deduplicate
  typeset -U domains
  echo "${domains[@]}"
}

# Get verbs for a domain from its dispatch table
_world_verbs() {
  local domain="$1"
  local -a verbs

  # Native domain verbs (hardcoded for speed)
  case "$domain" in
    network)   verbs=(reset enable disable remove restart) ;;
    service)   verbs=(restart enable disable set) ;;
    disk)      verbs=(clear reset add remove) ;;
    printer)   verbs=(clear restart set reset) ;;
    brew)      verbs=(add remove reset set) ;;
    process)   verbs=(kill remove set) ;;
    container) verbs=(enable disable restart remove add clear) ;;
    log)       verbs=() ;;
    *)
      # Plugin: parse dispatch.json
      local pdir="$(_world_plugins_dir)"
      local dispatch="$pdir/$domain/dispatch.json"
      if [[ -f "$dispatch" ]]; then
        verbs=($(grep '"verb"' "$dispatch" | sed 's/.*"verb"[[:space:]]*:[[:space:]]*"//;s/".*//' | sort -u))
      fi
      ;;
  esac

  echo "${verbs[@]}"
}

# Get await conditions for a domain
_world_conditions() {
  local domain="$1"
  local -a conds
  case "$domain" in
    network)   conds=(host_reachable dns_resolves internet_reachable port_open) ;;
    service)   conds=(healthy) ;;
    process)   conds=(running stopped port_free) ;;
    container) conds=(running healthy image_exists volume_exists) ;;
    disk)      conds=(writable) ;;
    brew)      conds=(installed) ;;
    printer)   conds=(prints) ;;
    browser)   conds=(loaded title_contains) ;;
    ssh)       conds=(connected) ;;
    home)      conds=(connected) ;;
  esac
  echo "${conds[@]}"
}

# Get observe targets for a domain
_world_observe_targets() {
  local domain="$1"
  case "$domain" in
    process)   echo "top_cpu top_mem" ;;
    network)   echo "dns internet_status proxy vpn" ;;
    disk)      echo "temp_usage" ;;
    container) echo "images volumes" ;;
  esac
}

# ─── Per-subcommand completions ───────────────────────────────────────────────

_world_complete_observe() {
  case $CURRENT in
    2)
      local -a doms=($(_world_domains))
      _describe -t domains 'domain' doms
      ;;
    3)
      local domain="${line[1]}"
      local -a targets=($(_world_observe_targets "$domain"))
      if (( ${#targets} )); then
        _describe -t targets 'target' targets
      fi
      ;;
  esac
  _arguments \
    '--since=[Time filter]:since:' \
    '--limit=[Max results]:limit:' \
    '--json[Force JSON]' \
    '--pretty[Human-readable]'
}

_world_complete_act() {
  case $CURRENT in
    2)
      local -a doms=($(_world_domains))
      _describe -t domains 'domain' doms
      ;;
    3|4)
      local domain="${line[1]}"
      local -a verbs=($(_world_verbs "$domain"))
      if (( ${#verbs} )); then
        _describe -t verbs 'verb' verbs
      fi
      ;;
  esac
  _arguments \
    '--dry-run[Preview without executing]' \
    '--json[Force JSON]' \
    '--pretty[Human-readable]'
}

_world_complete_await() {
  case $CURRENT in
    2)
      local -a doms=($(_world_domains))
      _describe -t domains 'domain' doms
      ;;
    3|4)
      local domain="${line[1]}"
      local -a conds=($(_world_conditions "$domain"))
      if (( ${#conds} )); then
        _describe -t conditions 'condition' conds
      fi
      ;;
  esac
  _arguments \
    '--timeout=[Max seconds]:timeout:' \
    '--json[Force JSON]' \
    '--pretty[Human-readable]'
}

_world_complete_sample() {
  case $CURRENT in
    2)
      local -a doms=($(_world_domains))
      _describe -t domains 'domain' doms
      ;;
    3)
      local domain="${line[1]}"
      local -a targets=($(_world_observe_targets "$domain"))
      if (( ${#targets} )); then
        _describe -t targets 'target' targets
      fi
      ;;
  esac
  _arguments \
    '--count=[Number of samples]:count:' \
    '--interval=[Interval]:interval:' \
    '--limit=[Max results]:limit:' \
    '--json[Force JSON]' \
    '--pretty[Human-readable]'
}

_world_complete_spec() {
  case $CURRENT in
    2)
      local -a doms=($(_world_domains))
      _describe -t domains 'domain' doms
      ;;
  esac
  _arguments \
    '--core[Core only, no add-ons]' \
    '--json[Force JSON]' \
    '--pretty[Human-readable]'
}

# ─── Main completion function ─────────────────────────────────────────────────

_world() {
  local -a commands
  commands=(
    'observe:Observe structured state'
    'o:Observe (alias)'
    'act:Act on the world'
    'a:Act (alias)'
    'await:Await a condition'
    'w:Await (alias)'
    'spec:Show domain spec'
    's:Spec (alias)'
    'sample:Sample over time'
    'tools:List tools and verbs'
    'addons:List add-ons'
    'completions:Generate shell completions'
  )

  _arguments -C \
    '--json[Force JSON output]' \
    '--pretty[Force human-readable output]' \
    '(-q --quiet)'{-q,--quiet}'[Exit code only]' \
    '(-h --help)'{-h,--help}'[Print help]' \
    '(-V --version)'{-V,--version}'[Print version]' \
    '1: :->command' \
    '*:: :->args' \
    && return

  case "$state" in
    command)
      _describe -t commands 'world command' commands
      ;;
    args)
      local cmd="$line[1]"
      case "$cmd" in
        observe|o)  _world_complete_observe ;;
        act|a)      _world_complete_act ;;
        await|w)    _world_complete_await ;;
        sample)     _world_complete_sample ;;
        spec|s)     _world_complete_spec ;;
        completions)
          local -a shells=(bash zsh fish)
          _describe -t shells 'shell' shells
          ;;
      esac
      ;;
  esac
}

# ─── Alias-specific completions ───────────────────────────────────────────────
# Each alias gets self-contained positional completion.
# wa <TAB> → domain, wa home <TAB> → verb, etc.

_world_alias_observe() {
  local -a doms=($(_world_domains))
  _arguments -C \
    '--since=[Time filter]:since:' \
    '--limit=[Max results]:limit:' \
    '--json[Force JSON]' \
    '--pretty[Human-readable]' \
    '(-q --quiet)'{-q,--quiet}'[Exit code only]' \
    "1:domain:(${doms[*]})" \
    '2:target:->target' \
    && return
  case "$state" in
    target)
      local -a targets=($(_world_observe_targets "${line[1]}"))
      (( ${#targets} )) && _describe -t targets 'target' targets
      ;;
  esac
}

_world_alias_act() {
  local -a doms=($(_world_domains))
  _arguments -C \
    '--dry-run[Preview without executing]' \
    '--json[Force JSON]' \
    '--pretty[Human-readable]' \
    '(-q --quiet)'{-q,--quiet}'[Exit code only]' \
    "1:domain:(${doms[*]})" \
    '2:target or verb:->tv2' \
    '3:verb:->tv3' \
    '*:args:' \
    && return
  case "$state" in
    tv2|tv3)
      local -a verbs=($(_world_verbs "${line[1]}"))
      (( ${#verbs} )) && _describe -t verbs 'verb' verbs
      ;;
  esac
}

_world_alias_spec() {
  local -a doms=($(_world_domains))
  _arguments \
    '--core[Core only, no add-ons]' \
    '--json[Force JSON]' \
    '--pretty[Human-readable]' \
    '(-q --quiet)'{-q,--quiet}'[Exit code only]' \
    "1:domain:(${doms[*]})"
}

_world_alias_await() {
  local -a doms=($(_world_domains))
  _arguments -C \
    '--timeout=[Max seconds]:timeout:' \
    '--json[Force JSON]' \
    '--pretty[Human-readable]' \
    '(-q --quiet)'{-q,--quiet}'[Exit code only]' \
    "1:domain:(${doms[*]})" \
    '2:target or condition:->tc2' \
    '3:condition:->tc3' \
    && return
  case "$state" in
    tc2|tc3)
      local -a conds=($(_world_conditions "${line[1]}"))
      (( ${#conds} )) && _describe -t conditions 'condition' conds
      ;;
  esac
}

_world_alias_sample() {
  local -a doms=($(_world_domains))
  _arguments -C \
    '--count=[Number of samples]:count:' \
    '--interval=[Interval]:interval:' \
    '--limit=[Max results]:limit:' \
    '--json[Force JSON]' \
    '--pretty[Human-readable]' \
    '(-q --quiet)'{-q,--quiet}'[Exit code only]' \
    "1:domain:(${doms[*]})" \
    '2:target:->target' \
    && return
  case "$state" in
    target)
      local -a targets=($(_world_observe_targets "${line[1]}"))
      (( ${#targets} )) && _describe -t targets 'target' targets
      ;;
  esac
}

compdef _world world
compdef _world_alias_observe wo
compdef _world_alias_act wa
compdef _world_alias_spec ws
compdef _world_alias_await ww
compdef _world_alias_sample wsample
