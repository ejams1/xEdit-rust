{ parity probe: x-003 }
unit P_x_003;

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
  d := 7 / 0;
  AddMessage('after');
end;
end.
