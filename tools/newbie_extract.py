"""Extracts the parameters of the Classic newbie quest families (Q10961..Q10990)
from their L2J Mobius Java sources and prints a Rust table."""
import re, sys, os

root = sys.argv[1]
first = ['Q10961_EffectiveTraining', 'Q10981_UnbearableWolvesHowling', 'Q10983_TroubledForest',
         'Q10985_CleaningUpTheGround', 'Q10987_PlunderedGraves', 'Q10989_DangerousPredators']
second = ['Q10962_NewHorizons', 'Q10982_SpiderHunt', 'Q10984_CollectSpiderweb', 'Q10986_SwampMonster',
          'Q10988_Conspiracy', 'Q10990_PoisonExtraction']


def load(q):
    src = open(os.path.join(root, q, q + '.java'), encoding='utf-8').read()
    ints = dict(re.findall(r'private static final int (\w+) = (\d+);', src))
    holders = {k: (int(a), int(b)) for k, a, b in re.findall(r'private static final ItemHolder (\w+) = new ItemHolder\((\d+), (\d+)\);', src)}
    title = re.sub(r'(?<=[a-z])(?=[A-Z])', ' ', q.split('_', 1)[1])
    qid = int(re.search(r'super\((\d+)\)', src).group(1))
    def ids(call):
        m = re.search(call + r'\(([^)]*)\)', src)
        return [int(ints[x.strip()]) if x.strip() in ints else int(x) for x in m.group(1).split(',')] if m else []
    tele = re.search(r'teleToLocation\((-?\d+), (-?\d+), (-?\d+)\)', src)
    msg = re.search(r'new ExShowScreenMessage\("([^"]*)"', src)
    cases = {}
    for m in re.finditer(r'case "([^"]+)":\s*\{(.*?)\n\t\t\t\}', src, re.S):
        cases[m.group(1)] = m.group(2)
    kill_target = re.search(r'killCount < (\d+)', src)
    collect = re.search(r'getQuestItemsCount\(killer, (\w+)\) >= (\d+)', src)
    talk = re.search(r'public String onTalk(.*)', src, re.S).group(1)
    pages = re.findall(r'htmltext = "([^"]+)"', talk)
    return dict(src=src, ints=ints, holders=holders, title=title, id=qid, start=ids('addStartNpc'), talk=ids('addTalkId'),
                kills=ids('addKillId'), tele=tele.groups() if tele else None, msg=msg.group(1) if msg else '',
                cases=cases, kill_target=int(kill_target.group(1)) if kill_target else 0,
                collect=(int(ints[collect.group(1)]), int(collect.group(2))) if collect else None, pages=pages,
                max_level=int(ints.get('MAX_LEVEL', 0)))


def gives(body, holders):
    out = []
    for name in re.findall(r'giveItems\(player, (\w+)\)', body):
        out.append(holders[name])
    return out


def rs_items(items):
    return '&[' + ', '.join(f'({a}, {b})' for a, b in items) + ']'


def rs_ids(ids):
    return '&[' + ', '.join(str(i) for i in ids) + ']'


print('// Generated from the datapack Java sources by tools/newbie_extract.py.')
print('pub static FIRST: &[FirstHunt] = &[')
for q in first:
    d = load(q)
    start_ev = [e for e, b in d['cases'].items() if 'startQuest' in b][0]
    reward_ev, reward_body = [(e, b) for e, b in d['cases'].items() if 'addExpAndSp' in b][0]
    exp, sp = re.search(r'addExpAndSp\(player, (\d+), (\d+)\)', reward_body).groups()
    soe = [h for k, h in d['holders'].items() if k.startswith('SOE_TO')][0]
    print(f'''    FirstHunt {{
        id: {d['id']},
        dir: "{q}",
        title: "{d['title']}",
        guide: {d['start'][0]},
        trader: {[t for t in d['talk'] if t != d['start'][0]][0]},
        kills: {rs_ids(d['kills'])},
        kill_target: {d['kill_target']},
        soe: {soe},
        max_level: {d['max_level']},
        teleport: [{', '.join(x + '.0' for x in d['tele'])}],
        message: "{d['msg']}",
        start_event: "{start_ev}",
        reward_event: "{reward_ev}",
        exp: {exp},
        sp: {sp},
        rewards: {rs_items(gives(reward_body, d['holders']))},
        pages: [{', '.join('"%s"' % p for p in d['pages'])}],
    }},'''.replace('[-', '[-').replace(', -', ', -'))
print('];')
print()
print('pub static SECOND: &[SecondHunt] = &[')
for q in second:
    d = load(q)
    start_ev = [e for e, b in d['cases'].items() if 'startQuest' in b][0]
    plain = [e for e, b in d['cases'].items() if re.fullmatch(r'\s*htmltext = event;\s*break;\s*', b)]
    next_q = re.search(r'case "NEXT_QUEST":\s*\{\s*htmltext = "([^"]+)"', d['src'])
    choices = []
    exp = sp = None
    for e, b in d['cases'].items():
        if 'addExpAndSp' in b:
            exp, sp = re.search(r'addExpAndSp\(player, (\d+), (\d+)\)', b).groups()
            choices.append((e, gives(b, d['holders'])))
    soe = [h for k, h in d['holders'].items() if k.startswith('SOE_TO')][0]
    tele = d['tele']
    print(f'''    SecondHunt {{
        id: {d['id']},
        dir: "{q}",
        title: "{d['title']}",
        giver: {d['start'][0]},
        captain: {[t for t in d['talk'] if t != d['start'][0]][0]},
        kills: {rs_ids(d['kills'])},
        kill_target: {d['kill_target']},
        collect: {('Some(%d, %d)' % d['collect']).replace('Some(', 'Some((').replace(')', '))') if d['collect'] else 'None'},
        soe: {soe},
        max_level: {d['max_level']},
        teleport: [{', '.join(x + '.0' for x in tele)}],
        message: "{d['msg']}",
        start_event: "{start_ev}",
        plain_events: &[{', '.join('"%s"' % e for e in plain)}],
        next_quest: "{next_q.group(1) if next_q else ''}",
        exp: {exp},
        sp: {sp},
        choices: &[{', '.join('("%s", %s)' % (e, rs_items(i)) for e, i in choices)}],
        pages: [{', '.join('"%s"' % p for p in d['pages'])}],
    }},''')
print('];')
