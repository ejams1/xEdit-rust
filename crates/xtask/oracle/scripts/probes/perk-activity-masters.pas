{
  Parity probe (issue #33): the ATAV JSON text and the virtual perk activity
  tree of a Starfield PERK through a masters update. Skill_Rejuvenation
  [PERK:0028AE13] of Starfield.esm is copied as an override and as a new
  record into a new file, the light module OldMars.esm is added as a master
  and the masters are sorted and cleaned; the ATAV text and the tree of the
  override are written after each step, those of the new record (which
  nothing reads before) only at the end.
}
unit PerkActivityMasters;

const
  cConfig = 'Ranks\[0]\Activity Tracker\Activities\[0]\Progression Configuration\';

procedure Show(aTag: string; aRecord: IInterface);
var
  e: IInterface;
begin
  AddMessage(aTag + ' ' + Name(aRecord) + ' ATAV: ' + GetElementEditValues(aRecord, cConfig + 'ATAV - Configuration'));
  e := ElementByPath(aRecord, cConfig + 'NULL - Activity Data\Actor Value Activity\Stream');
  if Assigned(e) then begin
    AddMessage(aTag + ' stream count ' + IntToStr(ElementCount(e)));
    if ElementCount(e) > 0 then
      AddMessage(aTag + ' stream[0] ' + GetEditValue(ElementByIndex(e, 0)));
  end else
    AddMessage(aTag + ' has no stream');
  AddMessage(aTag + ' repetitions ' + GetElementEditValues(aRecord, cConfig + 'NULL - Activity Data\Actor Value Activity\Data\Repetitions'));
end;

function Initialize: Integer;
var
  i: Integer;
  src, f, ov, nw: IInterface;
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
  f := AddNewFileName('Sf33Masters.esp', False);
  AddRequiredElementMasters(src, f, False, True);
  ov := wbCopyElementToFile(src, f, False, True);
  nw := wbCopyElementToFile(src, f, True, True);
  Show('copied override', ov);
  AddMasterIfMissing(f, 'OldMars.esm');
  Show('master added override', ov);
  SortMasters(f);
  Show('sorted override', ov);
  CleanMasters(f);
  Show('cleaned override', ov);
  Show('cleaned new', nw);
end;

end.
