{ parity probe: x-213 }
unit P_x_213;

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
  v := sl + 1;
  AddMessage('after');
end;
end.
