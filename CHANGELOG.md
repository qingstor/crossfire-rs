# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

# Unreleased

### Added

### Removed

### Changed

### Fixed

# [2.0.0] - 2025-06-27

### Added

- spsc module

- Benchmark suite written with criterion.

### Changed

- Refactor the API design. Unify sender and receiver types.

- Removal of macro rules and refactor ChannelShared into Enum, thus removal of generic type in channel shared struture.

- Removal of the spin lock in LockedWaker. Simplifing the logic without lossing performance.

- Rewrite the test cases with rstest.

### Removed

- Drop SelectSame module, because of hard to maintain, can be replace with future-select.

# [1.1.0] - 2025-06-19

### Changed

- Migrate repo

From <http://github.com/qingstor/crossfire-rs> to <https://github.com/frostyplanet/crossfire-rs>

- Change rust edition to 2024, re-format the code and fix warnnings.


# [1.0.1] - 2023-08-29

### Fixed

- Fix atomic ordering for ARM (Have been tested on some ARM deployment)

# [1.0.0] - 2022-12-03

### Changed

- Format all code and announcing v1.0

- I decided that x86_64 stable after one year test.

# [0.1.7] - 2021-08-22

### Fixed

- tx: Remove redundant old_waker.is_waked() on abandon

# [0.1.6] - 2021-08-21

### Fixed

- mpsc: Fix RxFuture old_waker.abandon in poll_item

# [0.1.5] - 2021-06-28

### Changed

- Replace deprecated compare_and_swap

### Fixed

- SelectSame: Fix close_handler last_index

- Fix fetch_add/sub ordering for ARM  (discovered on test hang)
