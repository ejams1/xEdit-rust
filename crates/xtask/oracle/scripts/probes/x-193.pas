{ parity probe: x-193 }
unit P_x_193;

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
  v := - 'x';
  AddMessage('after');
end;
end.
