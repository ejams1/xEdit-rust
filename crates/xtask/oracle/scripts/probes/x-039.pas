{ parity probe: x-039 }
unit P_x_039;

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
  b := 'x';
  AddMessage('after');
end;
end.
