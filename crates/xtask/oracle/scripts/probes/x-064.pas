{ parity probe: x-064 }
unit P_x_064;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  v := ElementByIndex(FileByIndex(0), 99);
  AddMessage('after');
end;
end.
