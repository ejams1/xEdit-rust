{
  Parity fixture (issue #23): the editor ID edit of a record whose name was
  read first. Upstream's quick init (GetEditorID) of a record whose
  subrecords all lie before QuickInitLimit runs the tail of Init, adds the
  required members as an internal edit and releases the elements; the next
  init collapses the invalidated storage from the elements it has (none),
  so the record loses its subrecords and is saved with the EDID the script
  adds alone. A record read through ElementByName first keeps its data and
  gains its required members. The AVIF CNAM AfterLoad zeroes a category
  above 3. The Copy lines pin the RTL's index clamping, the DelimitedText
  lines the StrictDelimiter parsing of TStrings.
}
unit EdidQuickInit;

var
  gFile: IInterface;

procedure Dump(const aTag: string; aRecord: IInterface);
var
  i: integer;
  s: string;
begin
  s := '';
  for i := 0 to Pred(ElementCount(aRecord)) do begin
    if s <> '' then
      s := s + ', ';
    s := s + Name(ElementByIndex(aRecord, i));
  end;
  AddMessage(aTag + ': ' + IntToStr(ElementCount(aRecord)) + ' [' + s + ']');
end;

procedure Probe(aFormID: Cardinal; aNameFirst: Boolean; const aNewID: string);
var
  rec, el: IInterface;
begin
  rec := RecordByFormID(gFile, aFormID, False);
  if aNameFirst then
    AddMessage('Processing: ' + Name(rec))
  else
    AddMessage('Processing: ' + IntToHex(aFormID, 8));
  el := ElementByName(rec, 'EDID - Editor ID');
  if Assigned(el) then
    AddMessage('EDID present')
  else
    AddMessage('EDID missing');
  Dump('before', rec);
  if not Assigned(el) then
    el := Add(rec, 'EDID', True);
  SetEditValue(el, aNewID);
  Dump('after', rec);
  AddMessage('EDID=' + GetElementEditValues(rec, 'EDID') + ' CNAM=' + GetElementEditValues(rec, 'CNAM'));
end;

function Initialize: integer;
var
  i: integer;
  f: IInterface;
  sl: TStringList;
begin
  Result := 0;
  for i := 0 to Pred(FileCount) do begin
    f := FileByIndex(i);
    if SameText(GetFileName(f), 'Skyrim.esm') then
      gFile := f;
  end;
  Probe($00000800, True, 'KwdNoColorNamed2');
  Probe($00000801, False, 'KwdNoColorPath2');
  Probe($00000802, True, 'KwdColor2');
  Probe($00000803, True, 'ActiBounds2');
  Probe($00000804, True, 'ActiFull2');
  Probe($00000805, True, 'AvifBig2');
  Probe($00000806, True, 'AvifSmall2');
  AddMessage('Copy0=' + Copy('abcdef', 0, 3));
  AddMessage('Copy-1=' + Copy('abcdef', -1, 3));
  AddMessage('Copy1=' + Copy('abcdef', 1, 3));
  AddMessage('Copy0big=' + Copy('abcdef', 0, 100));
  AddMessage('Copy7=' + Copy('abcdef', 7, 2));
  AddMessage('Copy6=' + Copy('abcdef', 6, 2));
  sl := TStringList.Create;
  sl.Delimiter := ',';
  sl.StrictDelimiter := True;
  sl.DelimitedText := 'Model\MODL,Male World Model, "a,b" ,,last';
  AddMessage('strict=' + IntToStr(sl.Count) + ' [' + sl.Text + ']');
  sl.StrictDelimiter := False;
  sl.DelimitedText := 'Model\MODL,Male World Model, "a,b" ,,last';
  AddMessage('loose=' + IntToStr(sl.Count) + ' [' + sl.Text + ']');
  sl.Free;
end;

end.
