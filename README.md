# Yata Cloud

An open-source CNAPP (Cloud-Native Application Protection Platform) written in Rust.

The initial focus is CSPM (Cloud Security Posture Management): a CLI that scans AWS accounts for security misconfigurations using declarative policies.

> **Status:** Early development. Not ready for production use.

## Related resources

- Policies (rules) are not included in `yata`. The official AWS policy set is distributed from [yata-rules](https://github.com/morryyrrom/yata-rules), and `yata` loads it at runtime from the directory given by `--policy-dir` or `YATA_POLICY_DIR`.
- Design documents are in [docs/](docs/), starting with the [architecture overview](docs/architecture/Architecture-Overview.ja.md) (Japanese; English versions are not written yet).
- To report a security vulnerability, see [SECURITY.md](SECURITY.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
