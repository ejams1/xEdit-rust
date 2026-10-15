{ parity probe: x-288 }
unit P_x_288;

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
  sl.Add(FileByIndex(0));
  AddMessage('after');
end;
end.
