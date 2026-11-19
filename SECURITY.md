# Security

diamond-stats does not take credentials. The MLB Stats API it calls is unauthenticated. The HTTP server defaults to `127.0.0.1` and is not meant to be exposed as a public bulk-data service.

## Reporting a vulnerability

Please use [GitHub private vulnerability reporting](https://github.com/Matt-mdlivingston/diamond-stats/security/advisories/new) rather than a public issue.

Include what you ran, the version or commit, and the impact. Do not attach cached API responses, Parquet files, or other bulk statistics. A short description of the affected request is enough.

There is no bug bounty.
