# Enable public feedback posting

Audience: server operators.

Use a fine-grained token restricted to `esper256/infuser` with
[Issues write permission](https://docs.github.com/en/rest/issues/issues#create-an-issue).
Store it in `<state_dir>/feedback.github-token`, mode 0600, owned by the Infuser
service user. The default path is `/var/lib/infuser/feedback.github-token`.
Never commit or serve the state directory. Rotation/removal takes effect on the
next request. Without the token, adults can review/dismiss but cannot publish.
The issue is posted by the token's account to the fixed Infuser repository.
No child or reviewing adult needs their own GitHub account.


Back up the state directory privately. Pending reports expire after 30 days while
the controller is running; copies in backups need their own retention policy.
Run only one controller per state directory.
