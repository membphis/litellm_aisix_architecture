# ZSH Git Branch Prompt Implementation Notes

## Changes Made
1. Added git prompt configuration to `~/.zshrc`:
   - `ZSH_THEME_GIT_PROMPT_PREFIX="("`
   - `ZSH_THEME_GIT_PROMPT_SUFFIX=") "`
   - `ZSH_THEME_GIT_PROMPT_DIRTY="*"`
   - `ZSH_THEME_GIT_PROMPT_CLEAN=""`
   - `BULLETTRAIN_GIT_PROMPT_CMD='$(git_prompt_info)'`

## Expected Behavior
- In git repo: `~/git/litellm_aisix_architecture (main) HH:MM:SS`
- Dirty repo: `~/git/litellm_aisix_architecture (main*) HH:MM:SS`
- Not in git repo: `~/git/litellm_aisix_architecture HH:MM:SS`

## Testing Results
- [x] Branch shows in parentheses
- [x] No branch shown outside git repos
- [x] Dirty state indicator (*) works
- [x] Colors match bullet-train theme

## Files Modified
- `/home/rain/.zshrc` - Added git prompt configuration after opencode PATH export
- Backup created: `/home/rain/.zshrc.backup.20260411_112841`

## Notes
- Configuration uses Oh My Zsh's `git_prompt_info` function
- Bullet-train theme automatically positions git segment between dir and time
- Green color is default from bullet-train theme