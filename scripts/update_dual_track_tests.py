from pathlib import Path
p = Path('/home/node/Developments/zenithsw/tests/simulation_test.rs')
s = p.read_text()
# Keep legacy fixtures explicitly on the conservative default track.
s = s.replace('            reputation_score:', '            execution_track: ExecutionTrack::FastLane,\n            reputation_score:')
s = s.replace('        reputation_score:', '        execution_track: ExecutionTrack::FastLane,\n        reputation_score:')
s = s.replace('                reputation_score:', '                execution_track: ExecutionTrack::FastLane,\n                reputation_score:')
s = s.replace('            whale_tier:', '            whale_tier:')
# ShadowSignal literals: insert after whale_tier expression when missing.
lines = s.splitlines()
out = []
for line in lines:
    out.append(line)
    if 'whale_tier:' in line:
        indent = line[:len(line)-len(line.lstrip())]
        # All existing signal fixtures use the default track.
        out.append(f'{indent}track: ExecutionTrack::FastLane,')
p.write_text('\n'.join(out) + ('\n' if s.endswith('\n') else ''))
print('Updated simulation fixtures')
