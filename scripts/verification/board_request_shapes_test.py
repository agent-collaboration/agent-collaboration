"""Apparatus checks use the real shared builders and current product decoders."""

from pathlib import Path

from scripts.verification.board_request_shapes import check_decoder, retired_fields


def test_board_journeys_have_no_retired_wire_fields() -> None:
    assert retired_fields(Path(__file__).parent) == []


def test_shared_builders_match_current_product_decoders() -> None:
    repository = Path(__file__).resolve().parents[2]
    decoder = (
        repository
        / "scripts/verification/board_upgrade_native/target/debug/board-request-shapes"
    )
    assert decoder.is_file(), "build the documented nested helper before this check"
    assert check_decoder(decoder) == {"decoded": 12, "retiredFieldControlsRejected": 24}
