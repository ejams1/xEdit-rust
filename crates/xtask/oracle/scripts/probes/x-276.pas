{ parity probe: x-276 }
unit P_x_276;

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
  v := UpperCase(sl);
  AddMessage('after');
end;
end.
