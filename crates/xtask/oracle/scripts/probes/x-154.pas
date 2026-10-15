{ parity probe: x-154 }
unit P_x_154;

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
  v := 5; v := v / 'x';
  AddMessage('after');
end;
end.
