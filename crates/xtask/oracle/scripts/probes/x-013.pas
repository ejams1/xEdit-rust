{ parity probe: x-013 }
unit P_x_013;

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
  s := 'abc'; s[0] := 'x';
  AddMessage('after');
end;
end.
