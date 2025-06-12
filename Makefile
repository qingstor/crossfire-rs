RUNTESTCASE = _run_test_case() {                                                  \
    case="$(filter-out $@,$(MAKECMDGOALS))";                                      \
    if [ -n "$${case}" ]; then                                                    \
        RUST_BACKTRACE=full cargo test $${case} -- --nocapture --test-threads=1;  \
    else                                                                          \
        RUST_BACKTRACE=full cargo test -- --nocapture --test-threads=1;           \
    fi  \
}

RUNBENCHCASE = _run_bench_case() {                                                  \
    case="$(filter-out $@,$(MAKECMDGOALS))";                                      \
    if [ -n "$${case}" ]; then                                                    \
        RUST_BACKTRACE=full cargo test $${case} --release -- --nocapture --test-threads=1;  \
    else                                                                          \
        echo should specify test case;                                            \
    fi  \
}


INSTALL_GITHOOKS = _install_githooks() {                \
	git config core.hooksPath ./git-hooks;              \
}

.PHONY: git-hooks
git-hooks:
	@$(INSTALL_GITHOOKS); _install_githooks

.PHONY: init
init: git-hooks

.PHONY: fmt
fmt: init
	cargo fmt

.PHONY: test
test: init
	@echo "Run test"
	@${RUNTESTCASE}; _run_test_case
	@echo "Done"

.PHONY: bench
bench:
	@${RUNBENCHCASE}; _run_bench_case

.PHONY: build
build: init
	cargo build

.DEFAULT_GOAL = build
