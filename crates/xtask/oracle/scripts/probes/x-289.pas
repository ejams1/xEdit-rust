{ parity probe: x-289 }
unit P_x_289;

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
  sl.Add(Null); AddMessage('[' + sl[1] + ']');
  AddMessage('after');
end;
end.
