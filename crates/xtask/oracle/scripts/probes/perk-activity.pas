{
  Parity probe (issue #33): the virtual perk activity tree of a Starfield
  PERK and its ATAV JSON text through a copy and through edits of the tree.
  Skill_Rejuvenation [PERK:0028AE13] of Starfield.esm carries an Actor Value
  activity in each of its ranks; it is copied as an override and as a new
  record into a new file, its Stream FormID and Repetitions are set in the
  virtual tree and its ATAV text directly, and the ATAV text and the tree
  are written after each step.
}
unit PerkActivity;

const
  cConfig = '\Activity Tracker\Activities\[0]\Progression Configuration\';

function RankPath(aRank: Integer): string;
begin
  Result := 'Ranks\[' + IntToStr(aRank) + ']' + cConfig;
end;

function TreePath(aRank: Integer; aPath: string): string;
begin
  Result := RankPath(aRank) + 'NULL - Activity Data\Actor Value Activity\' + aPath;
end;

procedure Show(aTag: string; aRecord: IInterface; aRank: Integer);
var
  e: IInterface;
begin
  AddMessage(aTag + ' rank ' + IntToStr(aRank) + ' ATAV: ' + GetElementEditValues(aRecord, RankPath(aRank) + 'ATAV - Configuration'));
  e := ElementByPath(aRecord, RankPath(aRank) + 'NULL - Activity Data');
  if not Assigned(e) then begin
    AddMessage(aTag + ' rank ' + IntToStr(aRank) + ' has no virtual tree');
    Exit;
  end;
  AddMessage(aTag + ' rank ' + IntToStr(aRank) + ' tree: ' + IntToStr(ElementCount(e)) + ' ' + Name(ElementByIndex(e, 0)));
  e := ElementByPath(aRecord, TreePath(aRank, 'Stream'));
  if Assigned(e) then begin
    AddMessage(aTag + ' rank ' + IntToStr(aRank) + ' stream count ' + IntToStr(ElementCount(e)));
    if ElementCount(e) > 0 then
      AddMessage(aTag + ' rank ' + IntToStr(aRank) + ' stream[0] ' + GetEditValue(ElementByIndex(e, 0)));
  end else
    AddMessage(aTag + ' rank ' + IntToStr(aRank) + ' has no stream');
  AddMessage(aTag + ' rank ' + IntToStr(aRank) + ' repetitions ' + GetElementEditValues(aRecord, TreePath(aRank, 'Data\Repetitions')));
  AddMessage(aTag + ' rank ' + IntToStr(aRank) + ' accumulate ' + GetElementEditValues(aRecord, TreePath(aRank, 'Data\AccumulateDelta')));
end;

function Initialize: Integer;
var
  i: Integer;
  src, f, ov, nw, held: IInterface;
begin
  Result := 0;
  src := nil;
  for i := 0 to Pred(FileCount) do
    if GetFileName(FileByIndex(i)) = 'Starfield.esm' then
      src := RecordByFormID(FileByIndex(i), $0028AE13, False);
  // The probe phase runs every probe over its synthetic master.
  if not Assigned(src) then begin
    AddMessage('Starfield.esm is not loaded');
    Exit;
  end;
  Show('source', src, 0);
  f := AddNewFileName('Sf33Probe.esp', False);
  AddRequiredElementMasters(src, f, False, True);
  ov := wbCopyElementToFile(src, f, False, True);
  Show('override', ov, 0);
  Show('override', ov, 1);
  held := ElementByPath(ov, TreePath(0, 'Stream\[0]'));
  SetNativeValue(held, $2E1);
  AddMessage('held after set ' + GetEditValue(held));
  AddMessage('held path after set ' + Path(held));
  Show('stream set', ov, 0);
  held := ElementByPath(ov, TreePath(0, 'Stream\[0]'));
  SetNativeValue(held, $2E1);
  AddMessage('held after second set ' + GetEditValue(held));
  Show('stream set again', ov, 0);
  SetElementEditValues(ov, 'Ranks\[0]\PRUC - Quantum Essence Upgrade Cost', '7');
  Show('plain set', ov, 0);
  SetElementEditValues(ov, TreePath(1, 'Data\Repetitions'), '5');
  Show('repetitions set', ov, 1);
  SetElementEditValues(ov, RankPath(2) + 'ATAV - Configuration', '{"Data":{"AccumulateDelta":"Positive","Repetitions":"20"},"Stream":["724"],"Type":"BGSActorValueActivity"}');
  Show('atav set', ov, 2);
  nw := wbCopyElementToFile(src, f, True, True);
  Show('as new', nw, 0);
end;

end.
