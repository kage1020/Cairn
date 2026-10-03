- *(lsp)* An unreadable frame was logged as "client closed stdin without `shutdown`" before the
  frame's own error, so the first line said the opposite of what happened. The session now says the
  client closed stdin only once the reader has returned without an error. When the reader or the
  writer failed before `shutdown`, that failure is the one line, and it ends "the session ended
  without `shutdown`".
