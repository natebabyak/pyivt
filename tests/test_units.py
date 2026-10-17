from pyivt._bytes import nextpow2
from pyivt.decode import bit_layout, value_trailer
from pyivt.tidy import label_depth, label_parent


def test_nextpow2():
    assert nextpow2(1) == 1
    assert nextpow2(9) == 16
    assert nextpow2(21) == 32


def test_value_trailer():
    assert value_trailer(0x88, 0x00, 0x08) == 0
    assert value_trailer(0xA8, 0x41, 0x08) == 10


def test_bit_layout_age_table():
    # Age-straddle in-page extents for the sample layout
    stride, rec = bit_layout([8, 4, 32, 2])
    assert rec == 256
    assert stride[-1] == 1


def test_label_hierarchy():
    labels = ["Total", "  With", "    Low", "  Without"]
    assert label_depth(labels) == [0, 1, 2, 1]
    parents = label_parent(labels)
    assert parents[0] is None
    assert parents[1] == 1
    assert parents[2] == 2
    assert parents[3] == 1
