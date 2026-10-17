{
  Parity #41 probe, the control of filter-topic-bind.pas: the Process walk
  of an editing session over Fallout3.esm and BrokenSteel.esm without a
  filter, logging the responses of BrokenSteel.esm in the order the
  navigation tree holds them. A difference here is not the filter's.
}
unit userscript;

function Initialize: Integer;
begin
  Result := 0;
end;

function Process(e: IInterface): Integer;
begin
  if (Signature(e) = 'INFO') and (GetFileName(GetFile(e)) = 'BrokenSteel.esm') then
    AddMessage(IntToHex(GetLoadOrderFormID(e), 8) + ' ' + GetFileName(GetFile(e)));
  Result := 0;
end;

end.
