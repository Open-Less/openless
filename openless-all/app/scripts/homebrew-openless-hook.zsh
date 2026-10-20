# Event-driven only: never override brew, schedule a timer, or install a LaunchAgent.
typeset -g _openless_brew_pending=0

_openless_brew_preexec() {
  emulate -L zsh
  local word
  local -i command_start=1 after_brew=0
  _openless_brew_pending=0
  for word in ${(z)2}; do
    word=${(Q)word}
    case "$word" in
      '&&'|'||'|';'|'|'|'&'|'('|')') command_start=1; after_brew=0; continue ;;
    esac
    if (( command_start )); then
      [[ "$word" == *=* || "$word" == (command|env|sudo|noglob|nocorrect) || "$word" == -* ]] && continue
      command_start=0
      [[ "${word:t}" == brew ]] && after_brew=1
    elif (( after_brew )); then
      [[ "$word" == -* ]] && continue
      [[ "$word" == (update|update-reset|upgrade) ]] && _openless_brew_pending=1
      after_brew=0
    fi
  done
  return 0
}

_openless_brew_precmd() {
  local command_status=$?
  if (( _openless_brew_pending )); then
    _openless_brew_pending=0
    if (( command_status == 0 )); then
      command node "$HOME/.local/share/openless/homebrew-openless-notify.mjs" --notify || true
    else
      print -r -- 'Homebrew 命令未成功，已跳过 OpenLess 更新检查。'
    fi
  fi
  return 0
}

if [[ -o interactive ]]; then
  autoload -Uz add-zsh-hook
  add-zsh-hook -d preexec _openless_brew_preexec
  add-zsh-hook -d precmd _openless_brew_precmd
  add-zsh-hook preexec _openless_brew_preexec
  add-zsh-hook precmd _openless_brew_precmd
fi
