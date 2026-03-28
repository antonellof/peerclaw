from peerclaw.client import PeerclawClient


def test_base_url_normalization():
    c = PeerclawClient("http://127.0.0.1:9000/")
    assert c._url("/api/status") == "http://127.0.0.1:9000/api/status"
    assert c._url("api/status") == "http://127.0.0.1:9000/api/status"


def test_yaml_merge_tmp_path(tmp_path):
    from peerclaw.yaml_util import load_crew_from_yaml_dir

    (tmp_path / "agents.yaml").write_text("agents:\n  - id: x\n")
    (tmp_path / "tasks.yaml").write_text("tasks:\n  - id: y\n    agent_id: x\n")
    s = load_crew_from_yaml_dir(tmp_path)
    assert s["agents"]
    assert s["tasks"]
