{ parity probe: x-047 }
unit P_x_047;

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
  v := sl; i := v + 1;
  AddMessage('after');
end;
end.
