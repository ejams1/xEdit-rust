{
  Parity fixture (issue #40): the unit-scoped wbSOUNAfterLoad of Fallout 3
  and New Vegas. The fixture master (`soun-after-load-fo3.esm` for
  Fallout 3, `soun-after-load-fnv.esm` for New Vegas, otherwise the same)
  holds four sound records: the old format with ANAM, GNAM and HNAM, the
  old format without them, the new SNDD format, and one without sound
  data. The load runs the AfterLoad of the unit (the GUI's does, editing
  allowed), so the old SNDX record becomes the SNDD one and the old
  subrecords feed the new members (or the definition's defaults fill
  them), while the record that already has SNDD and the one without sound
  data are left alone: with the wbDefinitionsCommon copy of the routine
  (which turns SNDD into SNDX) all four would come out differently. The
  script prints the elements and the values, edits a point of a curve and
  renames an editor ID, and the log lines and the saved bytes are compared
  with the oracle run.
}
unit SounAfterLoad;

var
  gFile: IInterface;

procedure Dump(const aTag: string; aFormID: Cardinal);
var
  rec, snd, curve: IInterface;
  i: integer;
  s: string;
begin
  rec := RecordByFormID(gFile, aFormID, False);
  if not Assigned(rec) then begin
    AddMessage(aTag + ' elements=<missing>');
    Exit;
  end;
  s := '';
  for i := 0 to Pred(ElementCount(rec)) do begin
    if s <> '' then
      s := s + ', ';
    s := s + Name(ElementByIndex(rec, i));
  end;
  AddMessage(aTag + ' elements=' + IntToStr(ElementCount(rec)) + ' [' + s + ']');
  snd := ElementBySignature(rec, 'SNDD');
  if Assigned(snd) then begin
    curve := ElementByName(snd, 'Attenuation Curve');
    s := '';
    for i := 0 to Pred(ElementCount(curve)) do begin
      if s <> '' then
        s := s + ' ';
      s := s + GetEditValue(ElementByIndex(curve, i));
    end;
    AddMessage(aTag + ' SNDD curve=[' + s + '] reverb=' + GetElementEditValues(snd, 'Reverb Attenuation Control')
      + ' priority=' + GetElementEditValues(snd, 'Priority')
      + ' attenuation=' + GetElementEditValues(snd, 'Minimum Attenuation Distance')
      + '/' + GetElementEditValues(snd, 'Maximum Attenuation Distance')
      + ' flags=' + GetElementEditValues(snd, 'Flags'));
  end else
    AddMessage(aTag + ' SNDD=<none>');
  snd := ElementBySignature(rec, 'SNDX');
  if Assigned(snd) then
    AddMessage(aTag + ' SNDX=<present>')
  else
    AddMessage(aTag + ' SNDX=<none>');
  AddMessage(aTag + ' ANAM=' + GetElementEditValues(rec, 'ANAM') + ' GNAM=' + GetElementEditValues(rec, 'GNAM')
    + ' HNAM=' + GetElementEditValues(rec, 'HNAM'));
end;

function Initialize: integer;
var
  rec, snd, curve: IInterface;
begin
  Result := 0;
  gFile := FileByIndex(0);
  Dump('old-full', $00000800);
  Dump('old-short', $00000801);
  Dump('new', $00000802);
  Dump('none', $00000803);
  // An edit of the converted record: the first point of the curve and the
  // editor ID.
  rec := RecordByFormID(gFile, $00000800, False);
  snd := ElementBySignature(rec, 'SNDD');
  curve := ElementByName(snd, 'Attenuation Curve');
  SetEditValue(ElementByIndex(curve, 0), '77');
  SetEditValue(ElementByName(rec, 'EDID - Editor ID'), 'ParityOldFullEdited');
  Dump('old-full-edited', $00000800);
  if Assigned(ElementBySignature(rec, 'ANAM')) then
    AddMessage('ANAM=<still present>')
  else
    AddMessage('ANAM=<removed>');
  AddMessage('EDID=' + GetElementEditValues(rec, 'EDID'));
end;
