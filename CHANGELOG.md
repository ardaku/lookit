# Changelog
All notable changes to Lookit! will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://github.com/AldaronLau/semver).

## [0.3.0] - 2023-03-18
### Changed
 - Update pasts to 0.14
 - Update smelling_salts to 0.12

## [0.2.0] - 2023-01-28
### Changed
 - Update smelling_salts to 0.11
 - Update pasts to 0.13
 - Rename `Lookit` to `Searcher`
 - Rename `It` to `Found`
 - Implement `Notifier` instead of `Future` for `Searcher`
 - Replace methods on `Found` with simplified functions returning `Device`:
   - `connect()`
   - `connect_input()`
   - `connect_output()`

## [0.1.1] - 2021-11-25
### Changed
 - Updated to Smelling Salts version 0.6

## [0.1.0] - 2021-08-30
### Added
 - `It` struct for opening devices.
 - `Lookit` Future for finding devices.
 - Linux implementation
