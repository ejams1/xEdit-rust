{ parity probe: x-102 }
unit P_x_102;

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
  AddMessage('tab' + #9 + 'bed');
  AddMessage('after');
end;
end.
