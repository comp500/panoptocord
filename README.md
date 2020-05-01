# panoptocord
Panopto -> Discord webhook bot, to show new recordings. Polls the OAuth API every 10 minutes, and retrieves data for each folder ID you give it.

Needs supplied valid refresh and access tokens, as it doesn't handle OAuth authentication.

Also an experiment in Rust async I/O and serialisation/deserialisation!
